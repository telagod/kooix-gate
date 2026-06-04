//! ADR-0006 v1 component-model host — wasmtime::component::bindgen! 生成。
//!
//! v0 的 `wasmtime_host.rs` 保留不动；本模块平行存在，供 Phase 4 运行时检测调度。

use crate::error::{WasmError, WasmResult};
use crate::host::{HookContext, WasmHostConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store};

wasmtime::component::bindgen!({
    world: "plugin",
    path: "wit/kooix-plugin.wit",
});

/// Store data holding per-invocation context for host function implementations.
pub(crate) struct HostState {
    secrets: Arc<HashMap<String, String>>,
    allowed_slots: Arc<std::collections::HashSet<String>>,
}

impl HostState {
    fn new(ctx: &HookContext) -> Self {
        Self {
            secrets: Arc::new(ctx.secrets.clone()),
            allowed_slots: Arc::new(ctx.allowed_slots.clone()),
        }
    }
}

impl kooix::plugin::host::Host for HostState {
    fn get_secret(
        &mut self,
        ref_: kooix::plugin::host::SecretRef,
    ) -> Result<kooix::plugin::host::SecretBytes, kooix::plugin::host::SecretError> {
        if !self.allowed_slots.contains(&ref_.slot) {
            metrics::counter!(
                "gate_plugin_wasm_secret_access_total",
                "outcome" => "denied"
            )
            .increment(1);
            return Err(kooix::plugin::host::SecretError::Denied(format!(
                "slot '{}' not in allowed_slots",
                ref_.slot
            )));
        }
        let Some(secret) = self.secrets.get(&ref_.slot) else {
            metrics::counter!(
                "gate_plugin_wasm_secret_access_total",
                "outcome" => "missing"
            )
            .increment(1);
            return Err(kooix::plugin::host::SecretError::Missing(ref_.slot));
        };
        metrics::counter!(
            "gate_plugin_wasm_secret_access_total",
            "outcome" => "ok"
        )
        .increment(1);
        Ok(kooix::plugin::host::SecretBytes {
            value: secret.as_bytes().to_vec(),
        })
    }

    fn log(&mut self, level: u8, message: String) {
        const MAX_LOG: usize = 1024;
        let truncated = message.len() > MAX_LOG;
        let msg = &message[..message.len().min(MAX_LOG)];
        match level {
            0 => tracing::trace!(plugin = true, truncated, "{msg}"),
            1 => tracing::debug!(plugin = true, truncated, "{msg}"),
            2 => tracing::info!(plugin = true, truncated, "{msg}"),
            3 => tracing::warn!(plugin = true, truncated, "{msg}"),
            4 => tracing::error!(plugin = true, truncated, "{msg}"),
            _ => tracing::debug!(plugin = true, level, truncated, "{msg}"),
        }
        metrics::counter!("gate_plugin_wasm_host_log_total", "level" => level.to_string())
            .increment(1);
    }

    fn record_metric(&mut self, name: String, value: f64) {
        let sanitized: String = name
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(64)
            .collect();
        if sanitized.is_empty() {
            return;
        }
        let final_name = format!("plugin_wasm_user_{sanitized}");
        metrics::gauge!(final_name).set(value);
    }

    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn nonce(&mut self, bytes: u32) -> Vec<u8> {
        let mut buf = vec![0u8; bytes as usize];
        getrandom::fill(&mut buf).expect("getrandom failed");
        buf
    }

    fn redact(&mut self, value: String) -> String {
        if value.len() <= 8 {
            return "*".repeat(value.len());
        }
        let visible = 4;
        format!("{}...{}", &value[..visible], "*".repeat(4))
    }
}

impl kooix::plugin::types::Host for HostState {}

/// Compiled component module entry for a channel.
struct ChannelComponent {
    component: Component,
    #[allow(dead_code)]
    sha256: String,
}

/// Component-model host (v1 ABI). Parallel to WasmtimeHost (v0 ABI).
pub struct ComponentHost {
    engine: Engine,
    config: WasmHostConfig,
    components: Arc<RwLock<HashMap<String, ChannelComponent>>>,
}

impl ComponentHost {
    pub fn new(config: WasmHostConfig) -> WasmResult<Self> {
        let mut wasm_config = Config::new();
        wasm_config.consume_fuel(true);
        wasm_config.wasm_component_model(true);
        let engine = Engine::new(&wasm_config)
            .map_err(|e| WasmError::Load(format!("component engine init: {e}")))?;
        Ok(Self {
            engine,
            config,
            components: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Load a WASM component (v1 ABI). Validates sha256 digest.
    pub async fn load_component(
        &self,
        channel_id: &str,
        module_bytes: &[u8],
        expected_sha256: &str,
    ) -> WasmResult<()> {
        let actual = sha256_hex(module_bytes);
        if !expected_sha256.is_empty() && actual != expected_sha256.to_ascii_lowercase() {
            return Err(WasmError::DigestMismatch {
                expected: expected_sha256.to_string(),
                actual,
            });
        }
        let component = Component::new(&self.engine, module_bytes)
            .map_err(|e| WasmError::Load(format!("component compile: {e}")))?;
        let mut components = self.components.write().await;
        components.insert(
            channel_id.to_string(),
            ChannelComponent {
                component,
                sha256: actual,
            },
        );
        tracing::info!(channel_id, "v1 component loaded");
        Ok(())
    }

    /// Invoke a transform hook on a loaded component.
    pub async fn invoke_transform(
        &self,
        channel_id: &str,
        hook: TransformHook,
        json: &str,
        ctx: HookContext,
    ) -> WasmResult<Option<TransformResult>> {
        let components = self.components.read().await;
        let entry = match components.get(channel_id) {
            Some(c) => c,
            None => return Ok(None),
        };

        let mut linker = Linker::new(&self.engine);
        Plugin::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |state| state)
            .map_err(|e| WasmError::Instantiate(format!("linker add_to_linker: {e}")))?;

        let mut store = Store::new(&self.engine, HostState::new(&ctx));
        let fuel = self.config.limits.max_cpu_ms.saturating_mul(1_000_000_000);
        store
            .set_fuel(fuel)
            .map_err(|e| WasmError::Instantiate(format!("set_fuel: {e}")))?;

        let plugin = Plugin::instantiate_async(&mut store, &entry.component, &linker)
            .await
            .map_err(|e| WasmError::Instantiate(format!("component instantiate: {e}")))?;

        let input = kooix::plugin::types::TransformInput {
            request_id: ctx.request_id.clone(),
            org_id: ctx.metadata.get("org_id").cloned().unwrap_or_default(),
            project_id: ctx.metadata.get("project_id").cloned(),
            channel_id: ctx.channel_id.clone(),
            model: ctx.model.clone(),
            json: json.to_string(),
        };

        let transform = plugin.kooix_plugin_transform();
        let result = match hook {
            TransformHook::Request => transform.call_transform_request(&mut store, &input),
            TransformHook::Response => transform.call_transform_response(&mut store, &input),
            TransformHook::StreamEvent => transform.call_transform_stream_event(&mut store, &input),
            TransformHook::FinishStream => transform.call_finish_stream(&mut store, &input),
        };

        match result {
            Ok(Ok(output)) => Ok(Some(TransformResult {
                json: output.json,
                metadata: output.metadata,
            })),
            Ok(Err(transform_err)) => {
                let msg = match transform_err {
                    kooix::plugin::types::TransformError::InvalidInput(s) => {
                        format!("invalid_input: {s}")
                    }
                    kooix::plugin::types::TransformError::Denied(s) => format!("denied: {s}"),
                    kooix::plugin::types::TransformError::UpstreamProtocol(s) => {
                        format!("upstream_protocol: {s}")
                    }
                    kooix::plugin::types::TransformError::Internal(s) => format!("internal: {s}"),
                };
                Err(WasmError::Call {
                    hook: hook.as_str(),
                    message: msg,
                })
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("fuel") {
                    Err(WasmError::Timeout {
                        limit_ms: self.config.limits.max_cpu_ms,
                    })
                } else {
                    Err(WasmError::Call {
                        hook: hook.as_str(),
                        message: msg,
                    })
                }
            }
        }
    }

    pub async fn unload_component(&self, channel_id: &str) -> WasmResult<()> {
        let mut components = self.components.write().await;
        components.remove(channel_id);
        tracing::info!(channel_id, "v1 component unloaded");
        Ok(())
    }
}

/// v1 transform hook types (richer than v0's 3 hooks).
#[derive(Debug, Clone, Copy)]
pub enum TransformHook {
    Request,
    Response,
    StreamEvent,
    FinishStream,
}

impl TransformHook {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransformHook::Request => "transform_request",
            TransformHook::Response => "transform_response",
            TransformHook::StreamEvent => "transform_stream_event",
            TransformHook::FinishStream => "finish_stream",
        }
    }
}

/// Successful transform result from a v1 component.
#[derive(Debug, Clone)]
pub struct TransformResult {
    pub json: String,
    pub metadata: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_host_creates_successfully() {
        let _host = ComponentHost::new(WasmHostConfig::default()).expect("engine init");
    }

    #[test]
    fn transform_hook_str_mapping() {
        assert_eq!(TransformHook::Request.as_str(), "transform_request");
        assert_eq!(TransformHook::Response.as_str(), "transform_response");
        assert_eq!(
            TransformHook::StreamEvent.as_str(),
            "transform_stream_event"
        );
        assert_eq!(TransformHook::FinishStream.as_str(), "finish_stream");
    }
}
