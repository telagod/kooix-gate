//! ADR-0003 v0 fallback policy：所有 hook 调用走此 wrapper。
//!
//! 行为：
//! - 模块未加载 → identity passthrough（return Ok(payload)）
//! - 调用成功 → return transform 后 payload
//! - 调用失败（Timeout / OOM / Call / Panic）→ `tracing::error!` + 降级到原 payload
//! - 全程上报 metric：`gate_plugin_wasm_calls_total{channel,hook,status}`

use crate::error::WasmError;
use crate::host::{HookContext, HookKind, WasmHost};
use bytes::Bytes;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

/// 调用 hook，全部错误降级为 identity passthrough；不向上 propagate。
pub async fn invoke_with_fallback(
    host: Arc<dyn WasmHost>,
    channel_id: &str,
    hook: HookKind,
    payload: Bytes,
    ctx: HookContext,
) -> Bytes {
    let payload_clone = payload.clone();
    let fut = host.invoke_hook(channel_id, hook, payload, ctx);

    // FutureExt::catch_unwind 捕获 wasm panic（理论上 wasmtime 不会让 host panic，
    // 但 host_log 等 closure 可能 panic；double safety）。
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(Ok(Some(transformed))) => {
            metrics::counter!(
                "gate_plugin_wasm_calls_total",
                "channel" => channel_id.to_string(),
                "hook" => hook.as_str(),
                "status" => "ok"
            )
            .increment(1);
            transformed
        }
        Ok(Ok(None)) => {
            // 模块未加载 — 不算失败
            metrics::counter!(
                "gate_plugin_wasm_calls_total",
                "channel" => channel_id.to_string(),
                "hook" => hook.as_str(),
                "status" => "no_module"
            )
            .increment(1);
            payload_clone
        }
        Ok(Err(err)) => {
            let status = match err {
                WasmError::Timeout { .. } => "timeout",
                WasmError::OutOfMemory { .. } => "oom",
                WasmError::DigestMismatch { .. } => "digest_mismatch",
                WasmError::Call { .. } => "call_error",
                WasmError::Load(_) => "load_error",
                WasmError::Instantiate(_) => "instantiate_error",
                WasmError::HostDenied(_) => "host_denied",
                WasmError::Panic(_) => "panic",
            };
            tracing::error!(
                channel_id,
                hook = hook.as_str(),
                error = %err,
                status,
                "wasm hook failed, fallback to identity"
            );
            metrics::counter!(
                "gate_plugin_wasm_calls_total",
                "channel" => channel_id.to_string(),
                "hook" => hook.as_str(),
                "status" => status
            )
            .increment(1);
            payload_clone
        }
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic>".to_string()
            };
            tracing::error!(
                channel_id,
                hook = hook.as_str(),
                panic = %msg,
                "wasm hook panicked, fallback to identity"
            );
            metrics::counter!(
                "gate_plugin_wasm_calls_total",
                "channel" => channel_id.to_string(),
                "hook" => hook.as_str(),
                "status" => "panic"
            )
            .increment(1);
            payload_clone
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasmHostConfig;
    use crate::host::WasmHost;
    use crate::wasmtime_host::WasmtimeHost;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct FailingHost;

    #[async_trait]
    impl WasmHost for FailingHost {
        async fn load_module(&self, _: &str, _: &[u8], _: &str) -> crate::WasmResult<()> {
            Ok(())
        }
        async fn invoke_hook(
            &self,
            _: &str,
            _: HookKind,
            _: Bytes,
            _: HookContext,
        ) -> crate::WasmResult<Option<Bytes>> {
            Err(WasmError::Timeout { limit_ms: 50 })
        }
        async fn unload_module(&self, _: &str) -> crate::WasmResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn fallback_returns_original_on_error() {
        let host: Arc<dyn WasmHost> = Arc::new(FailingHost);
        let payload = Bytes::from_static(b"identity-fallback");
        let result = invoke_with_fallback(
            host,
            "ch-1",
            HookKind::ChatRequest,
            payload.clone(),
            HookContext::default(),
        )
        .await;
        assert_eq!(result, payload);
    }

    #[tokio::test]
    async fn fallback_returns_transformed_on_success() {
        // 真 wasmtime host + 真模块 — 成功路径
        let host: Arc<dyn WasmHost> =
            Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
        let wat = r#"
            (module
              (memory (export "memory") 1)
              (func (export "gate_alloc") (param i32) (result i32) i32.const 4096)
              (func (export "chat_request_transform")
                (param $ptr i32) (param $len i32) (result i64)
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
            )
        "#;
        let module_bytes = wat::parse_str(wat).unwrap();
        let sha = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(&module_bytes);
            hex::encode(h.finalize())
        };
        host.load_module("ch-2", &module_bytes, &sha).await.unwrap();

        let payload = Bytes::from_static(b"transformed");
        let result = invoke_with_fallback(
            host,
            "ch-2",
            HookKind::ChatRequest,
            payload.clone(),
            HookContext::default(),
        )
        .await;
        assert_eq!(result, payload);
    }

    #[tokio::test]
    async fn fallback_returns_original_when_module_missing() {
        let host: Arc<dyn WasmHost> =
            Arc::new(WasmtimeHost::new(WasmHostConfig::default()).unwrap());
        let payload = Bytes::from_static(b"no-module");
        let result = invoke_with_fallback(
            host,
            "missing",
            HookKind::ChatRequest,
            payload.clone(),
            HookContext::default(),
        )
        .await;
        assert_eq!(result, payload);
    }
}
