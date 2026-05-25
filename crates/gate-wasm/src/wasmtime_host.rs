//! wasmtime 引擎托管的 WasmHost 实现。

use crate::error::{WasmError, WasmResult};
use crate::host::{HookContext, HookKind, WasmHost, WasmHostConfig};
use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::{Caller, Config, Engine, Extern, Linker, Module, Store};

/// 每 channel 一个 module instance（v0：no shared state）。
struct ChannelModule {
    module: Module,
    /// 加载时 sha256 校验通过的摘要；保留作 audit / observability 使用。
    #[allow(dead_code)]
    sha256: String,
}

/// ABI v0：
/// - module export `gate_alloc(size: i32) -> i32`：分配 linear memory，返回 ptr
/// - module export `<hook_name>(ptr_in: i32, len_in: i32) -> i64`：
///   返回 i64 = (ptr_out as u32) << 32 | (len_out as u32)
/// - host 通过 wasmtime memory 读 ptr_out / len_out 拿回 transform 后 payload
///
/// 0.4.24 接通 chat_request_transform 一条；0.4.25 加 response/stream。
pub struct WasmtimeHost {
    engine: Engine,
    config: WasmHostConfig,
    modules: Arc<RwLock<HashMap<String, ChannelModule>>>,
}

impl WasmtimeHost {
    pub fn new(config: WasmHostConfig) -> WasmResult<Self> {
        let mut wasm_config = Config::new();
        wasm_config.async_support(true);
        wasm_config.consume_fuel(true);
        let engine = Engine::new(&wasm_config)
            .map_err(|e| WasmError::Load(format!("engine init: {e}")))?;
        Ok(Self {
            engine,
            config,
            modules: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        hex::encode(h.finalize())
    }

    async fn invoke_hook_real(
        &self,
        module: &Module,
        hook: HookKind,
        payload: &[u8],
    ) -> WasmResult<Bytes> {
        // 1. 创建 store，注入 fuel
        let mut store: Store<()> = Store::new(&self.engine, ());
        // wasmtime fuel 单位：约 1 fuel ≈ 1 instr。50ms 内大致 5_000_000 instr 上限。
        // v0 用 max_cpu_ms × 1_000_000_000 给宽松 budget；v1 校准到真实 instr 数。
        let fuel = self.config.limits.max_cpu_ms.saturating_mul(1_000_000_000);
        store
            .set_fuel(fuel)
            .map_err(|e| WasmError::Instantiate(format!("set_fuel: {e}")))?;

        // 2. 准备 linker
        // 0.4.80（product-review B3a）：host_log 升级为真实读 wasm memory + 按
        // level 路由到 tracing 事件。其余 host fn（host_get_secret_slot /
        // host_record_metric）见 v0.4.81-82。
        let mut linker = Linker::new(&self.engine);
        linker
            .func_wrap(
                "env",
                "host_log",
                |mut caller: Caller<'_, ()>, level: i32, ptr: i32, len: i32| {
                    let memory = match caller.get_export("memory") {
                        Some(Extern::Memory(m)) => m,
                        _ => {
                            tracing::warn!("wasm host_log: plugin has no exported memory");
                            return;
                        }
                    };
                    let data = memory.data(&caller);
                    let p = ptr as usize;
                    let n = len as usize;
                    let bytes = if p.saturating_add(n) <= data.len() {
                        &data[p..p + n]
                    } else {
                        tracing::warn!(
                            ptr = p,
                            len = n,
                            mem_size = data.len(),
                            "wasm host_log: out-of-bounds string slice; dropping"
                        );
                        return;
                    };
                    // 截 1KB 防 plugin 把日志撑爆
                    const MAX_LOG: usize = 1024;
                    let truncated = bytes.len() > MAX_LOG;
                    let slice = &bytes[..bytes.len().min(MAX_LOG)];
                    let msg = String::from_utf8_lossy(slice);
                    // level 约定：0=trace 1=debug 2=info 3=warn 4=error；其它走 debug
                    match level {
                        0 => tracing::trace!(plugin = true, truncated, "{msg}"),
                        1 => tracing::debug!(plugin = true, truncated, "{msg}"),
                        2 => tracing::info!(plugin = true, truncated, "{msg}"),
                        3 => tracing::warn!(plugin = true, truncated, "{msg}"),
                        4 => tracing::error!(plugin = true, truncated, "{msg}"),
                        _ => tracing::debug!(plugin = true, level, truncated, "{msg}"),
                    }
                    metrics::counter!("gate_plugin_wasm_host_log_total", "level" => level.to_string()).increment(1);
                },
            )
            .map_err(|e| WasmError::Instantiate(format!("linker host_log: {e}")))?;

        // 3. 实例化（async）
        let instance = linker
            .instantiate_async(&mut store, module)
            .await
            .map_err(|e| WasmError::Instantiate(format!("instantiate: {e}")))?;

        // 4. 拿 memory + alloc
        let memory = match instance.get_export(&mut store, "memory") {
            Some(Extern::Memory(m)) => m,
            _ => return Err(WasmError::Call {
                hook: hook.as_str(),
                message: "module missing exported `memory`".into(),
            }),
        };
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "gate_alloc")
            .map_err(|e| WasmError::Call {
                hook: hook.as_str(),
                message: format!("missing gate_alloc: {e}"),
            })?;

        // 5. 写 payload
        let len_in = payload.len() as i32;
        let ptr_in = alloc
            .call_async(&mut store, len_in)
            .await
            .map_err(|e| WasmError::Call {
                hook: hook.as_str(),
                message: format!("alloc call: {e}"),
            })?;
        memory
            .write(&mut store, ptr_in as usize, payload)
            .map_err(|e| WasmError::Call {
                hook: hook.as_str(),
                message: format!("memory write: {e}"),
            })?;

        // 6. 调 hook
        let hook_fn = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, hook.as_str())
            .map_err(|e| WasmError::Call {
                hook: hook.as_str(),
                message: format!("missing export: {e}"),
            })?;
        let result = hook_fn
            .call_async(&mut store, (ptr_in, len_in))
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("fuel") {
                    WasmError::Timeout {
                        limit_ms: self.config.limits.max_cpu_ms,
                    }
                } else {
                    WasmError::Call {
                        hook: hook.as_str(),
                        message: msg,
                    }
                }
            })?;

        // 7. 读 ptr_out / len_out
        let ptr_out = (result >> 32) as i32;
        let len_out = result as i32;
        if len_out < 0 || ptr_out < 0 {
            return Err(WasmError::Call {
                hook: hook.as_str(),
                message: format!("invalid result encoding: ptr={ptr_out} len={len_out}"),
            });
        }
        let mut buf = vec![0u8; len_out as usize];
        memory
            .read(&store, ptr_out as usize, &mut buf)
            .map_err(|e| WasmError::Call {
                hook: hook.as_str(),
                message: format!("memory read: {e}"),
            })?;
        Ok(Bytes::from(buf))
    }
}

#[async_trait]
impl WasmHost for WasmtimeHost {
    async fn load_module(
        &self,
        channel_id: &str,
        module_bytes: &[u8],
        expected_sha256: &str,
    ) -> WasmResult<()> {
        let actual = Self::sha256_hex(module_bytes);
        if !expected_sha256.is_empty() && actual != expected_sha256.to_ascii_lowercase() {
            return Err(WasmError::DigestMismatch {
                expected: expected_sha256.to_string(),
                actual,
            });
        }
        let module = Module::new(&self.engine, module_bytes)
            .map_err(|e| WasmError::Load(format!("compile: {e}")))?;
        let mut modules = self.modules.write().await;
        modules.insert(
            channel_id.to_string(),
            ChannelModule {
                module,
                sha256: actual,
            },
        );
        tracing::info!(
            channel_id = channel_id,
            sha256 = %expected_sha256,
            "wasm module loaded"
        );
        Ok(())
    }

    async fn invoke_hook(
        &self,
        channel_id: &str,
        hook: HookKind,
        payload: Bytes,
        _ctx: HookContext,
    ) -> WasmResult<Option<Bytes>> {
        let modules = self.modules.read().await;
        let entry = match modules.get(channel_id) {
            Some(m) => m,
            None => return Ok(None),
        };

        // 0.4.25：3 个 hook 全部走真实路径，未 export 则 identity passthrough。
        if entry.module.get_export(hook.as_str()).is_none() {
            tracing::trace!(
                channel_id,
                hook = hook.as_str(),
                "hook not exported, identity passthrough"
            );
            return Ok(Some(payload));
        }
        let result = self.invoke_hook_real(&entry.module, hook, &payload).await?;
        Ok(Some(result))
    }

    async fn unload_module(&self, channel_id: &str) -> WasmResult<()> {
        let mut modules = self.modules.write().await;
        modules.remove(channel_id);
        tracing::info!(channel_id, "wasm module unloaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wasmtime_host_new_succeeds() {
        let host = WasmtimeHost::new(WasmHostConfig::default()).expect("engine init");
        host.unload_module("non-existent").await.unwrap();
    }

    #[tokio::test]
    async fn load_module_validates_sha256() {
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let correct_sha = WasmtimeHost::sha256_hex(&minimal_wasm);
        host.load_module("ch-1", &minimal_wasm, &correct_sha)
            .await
            .expect("load with correct sha");
        let err = host
            .load_module(
                "ch-2",
                &minimal_wasm,
                "0000000000000000000000000000000000000000000000000000000000000000",
            )
            .await
            .unwrap_err();
        match err {
            WasmError::DigestMismatch { .. } => {}
            other => panic!("expected DigestMismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn invoke_hook_returns_none_when_module_missing() {
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let result = host
            .invoke_hook(
                "missing",
                HookKind::ChatRequest,
                Bytes::from_static(b"{}"),
                HookContext::default(),
            )
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn chat_request_passthrough_when_hook_not_exported() {
        // 模块只有 (module)，没 export chat_request_transform → identity
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let sha = WasmtimeHost::sha256_hex(&minimal_wasm);
        host.load_module("ch-1", &minimal_wasm, &sha).await.unwrap();

        let payload = Bytes::from_static(b"{\"model\":\"gpt-4o\"}");
        let result = host
            .invoke_hook(
                "ch-1",
                HookKind::ChatRequest,
                payload.clone(),
                HookContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(result, Some(payload));
    }

    #[tokio::test]
    async fn chat_request_transforms_via_real_module() {
        // 真实 wasm：alloc 返回固定 buffer 起点；transform 把 input 拷到 buffer，
        // 返回 (ptr<<32 | len)。避免 memory.copy/bulk-memory 依赖。
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (func (export "gate_alloc") (param $size i32) (result i32)
                i32.const 4096)
              (func (export "chat_request_transform")
                (param $ptr i32) (param $len i32) (result i64)
                (local $i i32)
                (local.set $i (i32.const 0))
                (block $done
                  (loop $copy
                    (br_if $done (i32.ge_s (local.get $i) (local.get $len)))
                    (i32.store8
                      (i32.add (i32.const 4096) (local.get $i))
                      (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
                    (local.set $i (i32.add (local.get $i) (i32.const 1)))
                    (br $copy)))
                (i64.or
                  (i64.shl (i64.extend_i32_u (i32.const 4096)) (i64.const 32))
                  (i64.extend_i32_u (local.get $len))))
            )
        "#;
        let module_bytes = wat::parse_str(wat).unwrap();
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let sha = WasmtimeHost::sha256_hex(&module_bytes);
        host.load_module("ch-1", &module_bytes, &sha).await.unwrap();

        let payload = Bytes::from_static(b"hello-wasm-transform");
        let result = host
            .invoke_hook(
                "ch-1",
                HookKind::ChatRequest,
                payload.clone(),
                HookContext::default(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.as_ref(), payload.as_ref());
    }

    #[tokio::test]
    async fn chat_response_and_stream_chunk_invoke_real_module() {
        // 模块同时 export 三个 hook，同样的 identity copy 实现。
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (func $alloc (export "gate_alloc") (param i32) (result i32)
                i32.const 4096)
              (func $copy (param $ptr i32) (param $len i32) (result i64)
                (local $i i32)
                (block $done
                  (loop $copy
                    (br_if $done (i32.ge_s (local.get $i) (local.get $len)))
                    (i32.store8
                      (i32.add (i32.const 4096) (local.get $i))
                      (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
                    (local.set $i (i32.add (local.get $i) (i32.const 1)))
                    (br $copy)))
                (i64.or
                  (i64.shl (i64.extend_i32_u (i32.const 4096)) (i64.const 32))
                  (i64.extend_i32_u (local.get $len))))
              (func (export "chat_request_transform") (param i32 i32) (result i64)
                (call $copy (local.get 0) (local.get 1)))
              (func (export "chat_response_transform") (param i32 i32) (result i64)
                (call $copy (local.get 0) (local.get 1)))
              (func (export "stream_chunk_transform") (param i32 i32) (result i64)
                (call $copy (local.get 0) (local.get 1)))
            )
        "#;
        let module_bytes = wat::parse_str(wat).unwrap();
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let sha = WasmtimeHost::sha256_hex(&module_bytes);
        host.load_module("ch-2", &module_bytes, &sha).await.unwrap();

        for hook in [HookKind::ChatRequest, HookKind::ChatResponse, HookKind::StreamChunk] {
            let payload = Bytes::from(format!("payload-for-{}", hook.as_str()));
            let result = host
                .invoke_hook("ch-2", hook, payload.clone(), HookContext::default())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                result.as_ref(),
                payload.as_ref(),
                "hook {} mismatch",
                hook.as_str()
            );
        }
    }

    #[tokio::test]
    async fn other_hooks_remain_identity_in_v024() {
        // 0.4.25 起仍兼容：模块未 export 时 identity passthrough。
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let sha = WasmtimeHost::sha256_hex(&minimal_wasm);
        host.load_module("ch-1", &minimal_wasm, &sha).await.unwrap();
        let payload = Bytes::from_static(b"data");
        let resp = host
            .invoke_hook("ch-1", HookKind::ChatResponse, payload.clone(), HookContext::default())
            .await
            .unwrap();
        assert_eq!(resp, Some(payload.clone()));
        let stream = host
            .invoke_hook("ch-1", HookKind::StreamChunk, payload.clone(), HookContext::default())
            .await
            .unwrap();
        assert_eq!(stream, Some(payload));
    }
}

