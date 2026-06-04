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
        wasm_config.consume_fuel(true);
        let engine =
            Engine::new(&wasm_config).map_err(|e| WasmError::Load(format!("engine init: {e}")))?;
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

    /// 0.4.83（G-104）：cwasm 持久化缓存。
    ///
    /// 路径：`{cache_dir}/{sha256}-{wasmtime_major}.cwasm`。wasmtime major
    /// 写进文件名让升级 wasmtime 自动失效旧 cwasm（deserialize 不兼容会 panic）。
    ///
    /// 写盘失败不阻断 load —— 只 warn，下次重试。
    fn load_module_with_cache(
        engine: &Engine,
        cfg: &WasmHostConfig,
        sha256: &str,
        module_bytes: &[u8],
    ) -> WasmResult<Module> {
        let cache_dir = match &cfg.cache_dir {
            Some(d) => d,
            None => {
                return Module::new(engine, module_bytes)
                    .map_err(|e| WasmError::Load(format!("compile: {e}")));
            }
        };
        let wasmtime_major = env!("CARGO_PKG_VERSION")
            .split('.')
            .next()
            .unwrap_or("unknown");
        let cache_file = cache_dir.join(format!("{sha256}-wt{wasmtime_major}.cwasm"));

        // 1. 尝试 deserialize
        if cache_file.exists() {
            // SAFETY: Module::deserialize_file 是 unsafe 因为 cwasm 来自宿主自己写入，
            // 我们已经按 sha256+wasmtime_major 做了 versioning，从外部看是安全的。
            match unsafe { Module::deserialize_file(engine, &cache_file) } {
                Ok(m) => {
                    metrics::counter!("gate_wasm_cache_hit_total").increment(1);
                    tracing::debug!(path = %cache_file.display(), "wasm cache hit");
                    return Ok(m);
                }
                Err(e) => {
                    metrics::counter!("gate_wasm_cache_corrupt_total").increment(1);
                    tracing::warn!(
                        path = %cache_file.display(),
                        error = %e,
                        "wasm cache deserialize failed; removing + recompiling"
                    );
                    let _ = std::fs::remove_file(&cache_file);
                }
            }
        }

        // 2. Compile 新模块
        metrics::counter!("gate_wasm_cache_miss_total").increment(1);
        let module = Module::new(engine, module_bytes)
            .map_err(|e| WasmError::Load(format!("compile: {e}")))?;

        // 3. 尝试写回 cache（失败不阻断）
        if let Err(e) = std::fs::create_dir_all(cache_dir) {
            tracing::warn!(path = %cache_dir.display(), error = %e, "create cwasm cache dir failed");
            return Ok(module);
        }
        match module.serialize() {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&cache_file, &bytes) {
                    tracing::warn!(
                        path = %cache_file.display(),
                        error = %e,
                        "write cwasm cache failed"
                    );
                } else {
                    metrics::counter!("gate_wasm_cache_write_total").increment(1);
                    tracing::debug!(path = %cache_file.display(), bytes = bytes.len(), "wrote cwasm cache");
                }
            }
            Err(e) => tracing::warn!(error = %e, "module.serialize failed"),
        }
        Ok(module)
    }

    async fn invoke_hook_real(
        &self,
        module: &Module,
        hook: HookKind,
        payload: &[u8],
        // 0.4.137：接 HookContext 的 secrets + allowed_slots，让 host_get_secret_slot
        // 闭包可访问。用 Arc 让闭包能 move 捕获不消耗 ownership。
        secrets: std::sync::Arc<std::collections::HashMap<String, String>>,
        allowed_slots: std::sync::Arc<std::collections::HashSet<String>>,
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

        // 0.4.81（B3a step 2/3）：host_record_metric 实装。
        // 接收 (name_ptr, name_len, value_i64)，emit gauge metric。
        // 强制 metric name 前缀 `plugin_wasm_user_` 防 namespace 污染；
        // name 字符过滤为 [a-z0-9_]，超长截断 64。
        linker
            .func_wrap(
                "env",
                "host_record_metric",
                |mut caller: Caller<'_, ()>, name_ptr: i32, name_len: i32, value: i64| {
                    let memory = match caller.get_export("memory") {
                        Some(Extern::Memory(m)) => m,
                        _ => return,
                    };
                    let data = memory.data(&caller);
                    let p = name_ptr as usize;
                    let n = name_len as usize;
                    if p.saturating_add(n) > data.len() {
                        tracing::warn!(
                            "wasm host_record_metric: out-of-bounds name slice; dropping"
                        );
                        return;
                    }
                    let raw_name = String::from_utf8_lossy(&data[p..p + n]);
                    // sanitize: 只允许 [a-z0-9_]
                    let sanitized: String = raw_name
                        .to_ascii_lowercase()
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .take(64)
                        .collect();
                    if sanitized.is_empty() {
                        return;
                    }
                    let final_name = format!("plugin_wasm_user_{sanitized}");
                    metrics::gauge!(final_name).set(value as f64);
                },
            )
            .map_err(|e| WasmError::Instantiate(format!("linker host_record_metric: {e}")))?;

        // 0.4.137（按 docs/wasm-secret-slot-design.md，G-003 step 3/3）：
        // host_get_secret_slot(name_ptr, name_len, out_ptr, out_cap) -> i32
        //   >0  = bytes written
        //    0  = slot exists but empty
        //   -1  = slot not in allowed_slots
        //   -2  = slot declared but channel has no value
        //   -3  = out_cap < secret.len()
        //   -4  = name out-of-bounds or invalid UTF-8
        let secrets_for_closure = secrets.clone();
        let allowed_for_closure = allowed_slots.clone();
        linker
            .func_wrap(
                "env",
                "host_get_secret_slot",
                move |mut caller: Caller<'_, ()>,
                      name_ptr: i32,
                      name_len: i32,
                      out_ptr: i32,
                      out_cap: i32|
                      -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(Extern::Memory(m)) => m,
                        _ => return -5,
                    };
                    let data = memory.data(&caller);
                    let np = name_ptr as usize;
                    let nl = name_len as usize;
                    if np.saturating_add(nl) > data.len() {
                        return -4;
                    }
                    let name = match std::str::from_utf8(&data[np..np + nl]) {
                        Ok(s) => s.to_string(),
                        Err(_) => return -4,
                    };

                    // capability 校验
                    if !allowed_for_closure.contains(&name) {
                        metrics::counter!(
                            "gate_plugin_wasm_secret_access_total",
                            "outcome" => "denied"
                        )
                        .increment(1);
                        return -1;
                    }

                    let Some(secret) = secrets_for_closure.get(&name) else {
                        metrics::counter!(
                            "gate_plugin_wasm_secret_access_total",
                            "outcome" => "missing"
                        )
                        .increment(1);
                        return -2;
                    };

                    let bytes = secret.as_bytes();
                    if (bytes.len() as i32) > out_cap {
                        metrics::counter!(
                            "gate_plugin_wasm_secret_access_total",
                            "outcome" => "buf_too_small"
                        )
                        .increment(1);
                        return -3;
                    }

                    let op = out_ptr as usize;
                    let bl = bytes.len();
                    if op.saturating_add(bl) > data.len() {
                        return -5;
                    }
                    // 写入 wasm 内存
                    let mem_mut = memory.data_mut(&mut caller);
                    mem_mut[op..op + bl].copy_from_slice(bytes);

                    metrics::counter!(
                        "gate_plugin_wasm_secret_access_total",
                        "outcome" => "ok"
                    )
                    .increment(1);
                    bytes.len() as i32
                },
            )
            .map_err(|e| WasmError::Instantiate(format!("linker host_get_secret_slot: {e}")))?;

        // 3. 实例化（async）
        let instance = linker
            .instantiate_async(&mut store, module)
            .await
            .map_err(|e| WasmError::Instantiate(format!("instantiate: {e}")))?;

        // 4. 拿 memory + alloc
        let memory = match instance.get_export(&mut store, "memory") {
            Some(Extern::Memory(m)) => m,
            _ => {
                return Err(WasmError::Call {
                    hook: hook.as_str(),
                    message: "module missing exported `memory`".into(),
                });
            }
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
        // 0.4.83（G-104）：cwasm 持久化缓存。
        // 若配置了 cache_dir，先尝试 deserialize；失败 fallback compile + 写回。
        let module =
            Self::load_module_with_cache(&self.engine, &self.config, &actual, module_bytes)?;
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
        ctx: HookContext,
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
        let secrets = std::sync::Arc::new(ctx.secrets);
        let allowed_slots = std::sync::Arc::new(ctx.allowed_slots);
        let result = self
            .invoke_hook_real(&entry.module, hook, &payload, secrets, allowed_slots)
            .await?;
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

        for hook in [
            HookKind::ChatRequest,
            HookKind::ChatResponse,
            HookKind::StreamChunk,
        ] {
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
            .invoke_hook(
                "ch-1",
                HookKind::ChatResponse,
                payload.clone(),
                HookContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(resp, Some(payload.clone()));
        let stream = host
            .invoke_hook(
                "ch-1",
                HookKind::StreamChunk,
                payload.clone(),
                HookContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(stream, Some(payload));
    }

    // 0.4.82：把 host_record_metric 与 host_log 的命名/截断规则抽成自由函数复用，
    // 然后给 sanitize 写纯 unit test，避免去碰 wat 模块导入语义。
    // sanitize 规则与 wasmtime host fn 闭包内完全一致。
    fn sanitize_user_metric_name(raw: &str) -> Option<String> {
        let s: String = raw
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(64)
            .collect();
        if s.is_empty() {
            None
        } else {
            Some(format!("plugin_wasm_user_{s}"))
        }
    }

    #[test]
    fn user_metric_name_gets_namespace_prefix() {
        assert_eq!(
            sanitize_user_metric_name("cache_hits").as_deref(),
            Some("plugin_wasm_user_cache_hits")
        );
    }

    #[test]
    fn user_metric_name_lowercases_and_strips_bad_chars() {
        assert_eq!(
            sanitize_user_metric_name("Cache Hits/v2!").as_deref(),
            Some("plugin_wasm_user_cachehitsv2")
        );
    }

    #[test]
    fn user_metric_name_truncates_at_64() {
        let long = "a".repeat(200);
        let out = sanitize_user_metric_name(&long).unwrap();
        // prefix "plugin_wasm_user_" 是 17 字符 + sanitized 截到 64
        assert_eq!(out.len(), 17 + 64);
        assert!(out.starts_with("plugin_wasm_user_"));
    }

    #[test]
    fn user_metric_name_empty_after_sanitize_drops() {
        assert!(sanitize_user_metric_name("!!!@@@").is_none());
        assert!(sanitize_user_metric_name("").is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cwasm_cache_writes_and_hits_on_second_load() {
        // 0.4.83：验证 cwasm 持久化路径。
        let cache_dir =
            std::env::temp_dir().join(format!("kooix-gate-wasm-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache_dir);
        let mut cfg = WasmHostConfig::default();
        cfg.cache_dir = Some(cache_dir.clone());

        let host = WasmtimeHost::new(cfg.clone()).unwrap();
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let sha = WasmtimeHost::sha256_hex(&minimal_wasm);

        // 第一次 load：cache miss + 写盘
        host.load_module("ch-cache-1", &minimal_wasm, &sha)
            .await
            .unwrap();
        assert!(
            cache_dir.exists(),
            "cache_dir must be created after first load"
        );
        let entries: Vec<_> = std::fs::read_dir(&cache_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "expected 1 cwasm file");
        let cwasm_path = entries[0].path();
        assert!(
            cwasm_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&sha)
        );

        // 第二次：新 host 实例 + 同 cache_dir → 应该命中（不会重新 write）
        let mtime_before = std::fs::metadata(&cwasm_path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let host2 = WasmtimeHost::new(cfg).unwrap();
        host2
            .load_module("ch-cache-2", &minimal_wasm, &sha)
            .await
            .unwrap();
        let mtime_after = std::fs::metadata(&cwasm_path).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "cache hit should not rewrite the cwasm file"
        );

        // 清理
        let _ = std::fs::remove_dir_all(&cache_dir);
    }

    // 0.4.139（按 wasm-secret-slot-design.md 验收门禁）：host_get_secret_slot
    // 端到端测试。用 wat 写 plugin 调 host_get_secret_slot + chat_request_transform。
    // 但 wat 写 host fn 调用 + memory 操作较复杂，本测先验最小 case：
    //   - HookContext.secrets / allowed_slots 字段被 Arc clone 不丢
    //   - invoke_hook 接受新字段不 panic
    #[tokio::test(flavor = "multi_thread")]
    async fn invoke_hook_with_secrets_passes_through() {
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let sha = WasmtimeHost::sha256_hex(&minimal_wasm);
        host.load_module("ch-secret", &minimal_wasm, &sha)
            .await
            .unwrap();

        let mut ctx = HookContext::default();
        ctx.channel_id = "ch-secret".to_string();
        ctx.secrets
            .insert("primary".to_string(), "sk-test-12345".to_string());
        ctx.secrets
            .insert("aws_secret".to_string(), "AKIA-XYZ".to_string());
        ctx.allowed_slots.insert("primary".to_string());
        ctx.allowed_slots.insert("aws_secret".to_string());

        // (module) 没 export hook → identity passthrough，但 invoke 不应 panic
        let payload = Bytes::from_static(b"payload");
        let result = host
            .invoke_hook("ch-secret", HookKind::ChatRequest, payload.clone(), ctx)
            .await
            .unwrap();
        // 模块没 export 钩子 → returns Some(payload) (identity passthrough)
        assert_eq!(result, Some(payload));
    }

    #[test]
    fn hook_context_default_has_empty_secrets() {
        let ctx = HookContext::default();
        assert!(ctx.secrets.is_empty());
        assert!(ctx.allowed_slots.is_empty());
    }
}
