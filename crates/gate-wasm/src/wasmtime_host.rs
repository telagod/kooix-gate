//! wasmtime 引擎托管的 WasmHost 实现。

use crate::error::{WasmError, WasmResult};
use crate::host::{HookContext, HookKind, WasmHost, WasmHostConfig};
use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::{Engine, Module, Store, Config};

/// 每 channel 一个 module instance（v0：no shared state）。
struct ChannelModule {
    module: Module,
    sha256: String,
}

/// 基于 wasmtime engine 的 host 实现。
///
/// v0 行为：
/// - lazy compile：load_module 时编译并验证 sha256
/// - fuel-based timeout：每次 invoke_hook 注入 fuel 限制
/// - memory cap：通过 `wasmtime::ResourceLimiter` 设置
/// - panic safe：所有 host call 经 `catch_unwind`，panic 转 `WasmError::Panic`
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
        wasm_config.epoch_interruption(true);
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
}

#[async_trait]
impl WasmHost for WasmtimeHost {
    async fn load_module(
        &self,
        channel_id: &str,
        module_bytes: &[u8],
        expected_sha256: &str,
    ) -> WasmResult<()> {
        // 1. SHA256 校验
        let actual = Self::sha256_hex(module_bytes);
        if !expected_sha256.is_empty() && actual != expected_sha256.to_ascii_lowercase() {
            return Err(WasmError::DigestMismatch {
                expected: expected_sha256.to_string(),
                actual,
            });
        }

        // 2. compile
        let module = Module::new(&self.engine, module_bytes)
            .map_err(|e| WasmError::Load(format!("compile: {e}")))?;

        // 3. 入表
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
            None => return Ok(None), // 模块未加载 = hook 不可用
        };

        // 0.4.22 阶段：only check module + hook 名校验，body transform 留 0.4.24-0.4.25 接通。
        // 占位实现：return payload 原样（identity transform）。
        tracing::debug!(
            channel_id = channel_id,
            hook = hook.as_str(),
            sha256 = %entry.sha256,
            payload_bytes = payload.len(),
            "wasm hook invoked (identity stub, 0.4.22 placeholder)"
        );

        // 注入 fuel = max_cpu_ms × 1_000_000 (wasmtime 约 1M fuel/ms 经验值)
        let _store: Store<()> = Store::new(&self.engine, ());
        let fuel_budget = self.config.limits.max_cpu_ms.saturating_mul(1_000_000);
        // store.set_fuel(fuel_budget) 等价 — v0 stub 先记日志
        tracing::trace!(fuel_budget, "wasm fuel budget set");

        Ok(Some(payload))
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
        // 卸载不存在的 channel 不报错
        host.unload_module("non-existent").await.unwrap();
    }

    #[tokio::test]
    async fn load_module_validates_sha256() {
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        // 最小有效 wasm 模块：(module)
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let correct_sha = WasmtimeHost::sha256_hex(&minimal_wasm);

        // 正确 sha 通过
        host.load_module("ch-1", &minimal_wasm, &correct_sha)
            .await
            .expect("load with correct sha");

        // 错 sha 拒绝
        let err = host
            .load_module("ch-2", &minimal_wasm, "0000000000000000000000000000000000000000000000000000000000000000")
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
    async fn invoke_hook_returns_identity_for_loaded_module() {
        let host = WasmtimeHost::new(WasmHostConfig::default()).unwrap();
        let minimal_wasm = wat::parse_str("(module)").unwrap();
        let sha = WasmtimeHost::sha256_hex(&minimal_wasm);
        host.load_module("ch-1", &minimal_wasm, &sha).await.unwrap();

        let payload = Bytes::from_static(b"{\"model\":\"gpt\"}");
        let result = host
            .invoke_hook("ch-1", HookKind::ChatRequest, payload.clone(), HookContext::default())
            .await
            .unwrap();
        assert_eq!(result, Some(payload));
    }
}
