//! ADR-0006 Phase 4: unified dispatcher — auto-detect v0 core module vs v1 component.

use crate::component_host::{ComponentHost, TransformHook, TransformResult};
use crate::error::{WasmError, WasmResult};
use crate::host::{HookContext, HookKind, WasmHost, WasmHostConfig};
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// WASM binary format detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmFormat {
    /// Core module (v0 ABI): magic `\0asm` + version 1
    CoreModule,
    /// Component (v1 ABI): magic `\0asm` + component layer marker
    Component,
}

/// Detect whether a WASM binary is a core module or a component.
pub fn detect_format(bytes: &[u8]) -> WasmResult<WasmFormat> {
    if bytes.len() < 8 {
        return Err(WasmError::Load("wasm binary too short".into()));
    }
    if &bytes[0..4] != b"\0asm" {
        return Err(WasmError::Load("invalid wasm magic bytes".into()));
    }
    // Core module: version field is 0x01_00_00_00 (little-endian u32 = 1)
    // Component:   version field is 0x0d_00_01_00 (layer=13, version=1)
    let version_byte = bytes[4];
    match version_byte {
        0x01 => Ok(WasmFormat::CoreModule),
        0x0d => Ok(WasmFormat::Component),
        other => Err(WasmError::Load(format!(
            "unknown wasm version byte: 0x{other:02x}"
        ))),
    }
}

/// Track which format each channel uses.
#[derive(Debug, Clone, Copy)]
enum ChannelFormat {
    V0,
    V1,
}

/// Unified WASM host: auto-dispatches between v0 (WasmtimeHost) and v1 (ComponentHost).
#[allow(deprecated)]
pub struct UnifiedWasmHost {
    v0: crate::wasmtime_host::WasmtimeHost,
    v1: ComponentHost,
    formats: Arc<RwLock<HashMap<String, ChannelFormat>>>,
}

impl UnifiedWasmHost {
    #[allow(deprecated)]
    pub fn new(config: WasmHostConfig) -> WasmResult<Self> {
        let v0 = crate::wasmtime_host::WasmtimeHost::new(config.clone())?;
        let v1 = ComponentHost::new(config)?;
        Ok(Self {
            v0,
            v1,
            formats: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Invoke a v1 transform hook directly (for callers that know they have a component).
    pub async fn invoke_v1_transform(
        &self,
        channel_id: &str,
        hook: TransformHook,
        json: &str,
        ctx: HookContext,
    ) -> WasmResult<Option<TransformResult>> {
        self.v1.invoke_transform(channel_id, hook, json, ctx).await
    }
}

#[async_trait]
impl WasmHost for UnifiedWasmHost {
    async fn load_module(
        &self,
        channel_id: &str,
        module_bytes: &[u8],
        expected_sha256: &str,
    ) -> WasmResult<()> {
        let format = detect_format(module_bytes)?;
        match format {
            WasmFormat::CoreModule => {
                self.v0
                    .load_module(channel_id, module_bytes, expected_sha256)
                    .await?;
                tracing::info!(channel_id, format = "v0", "loaded core module");
            }
            WasmFormat::Component => {
                self.v1
                    .load_component(channel_id, module_bytes, expected_sha256)
                    .await?;
                tracing::info!(channel_id, format = "v1", "loaded component");
            }
        }
        let mut formats = self.formats.write().await;
        formats.insert(
            channel_id.to_string(),
            match format {
                WasmFormat::CoreModule => ChannelFormat::V0,
                WasmFormat::Component => ChannelFormat::V1,
            },
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
        let formats = self.formats.read().await;
        let format = match formats.get(channel_id) {
            Some(f) => *f,
            None => return Ok(None),
        };
        drop(formats);

        match format {
            ChannelFormat::V0 => self.v0.invoke_hook(channel_id, hook, payload, ctx).await,
            ChannelFormat::V1 => {
                let v1_hook = match hook {
                    HookKind::ChatRequest => TransformHook::Request,
                    HookKind::ChatResponse => TransformHook::Response,
                    HookKind::StreamChunk => TransformHook::StreamEvent,
                };
                let json = std::str::from_utf8(&payload).map_err(|e| WasmError::Call {
                    hook: hook.as_str(),
                    message: format!("payload is not UTF-8: {e}"),
                })?;
                match self
                    .v1
                    .invoke_transform(channel_id, v1_hook, json, ctx)
                    .await?
                {
                    Some(result) => Ok(Some(Bytes::from(result.json))),
                    None => Ok(None),
                }
            }
        }
    }

    async fn unload_module(&self, channel_id: &str) -> WasmResult<()> {
        let mut formats = self.formats.write().await;
        let format = formats.remove(channel_id);
        drop(formats);

        match format {
            Some(ChannelFormat::V0) => self.v0.unload_module(channel_id).await,
            Some(ChannelFormat::V1) => self.v1.unload_component(channel_id).await,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_core_module() {
        let core = wat::parse_str("(module)").unwrap();
        assert_eq!(detect_format(&core).unwrap(), WasmFormat::CoreModule);
    }

    #[test]
    fn detect_too_short() {
        assert!(detect_format(b"").is_err());
        assert!(detect_format(b"\0asm").is_err());
    }

    #[test]
    fn detect_bad_magic() {
        assert!(detect_format(b"not_wasm").is_err());
    }

    #[test]
    fn detect_component_byte() {
        // Component magic: \0asm + 0x0d 0x00 0x01 0x00
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(detect_format(&bytes).unwrap(), WasmFormat::Component);
    }

    #[tokio::test]
    async fn unified_host_loads_v0_module() {
        let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
        let module = wat::parse_str("(module)").unwrap();
        let sha = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(&module))
        };
        host.load_module("ch-unified-v0", &module, &sha)
            .await
            .unwrap();
        let formats = host.formats.read().await;
        assert!(matches!(
            formats.get("ch-unified-v0"),
            Some(ChannelFormat::V0)
        ));
    }

    #[tokio::test]
    async fn unified_host_invoke_v0_passthrough() {
        let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
        let module = wat::parse_str("(module)").unwrap();
        let sha = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(&module))
        };
        host.load_module("ch-v0", &module, &sha).await.unwrap();
        let payload = Bytes::from_static(b"test-payload");
        let result = host
            .invoke_hook(
                "ch-v0",
                HookKind::ChatRequest,
                payload.clone(),
                HookContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(result, Some(payload));
    }

    #[tokio::test]
    async fn unified_host_unload_v0() {
        let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
        let module = wat::parse_str("(module)").unwrap();
        let sha = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(&module))
        };
        host.load_module("ch-unload", &module, &sha).await.unwrap();
        host.unload_module("ch-unload").await.unwrap();
        let formats = host.formats.read().await;
        assert!(formats.get("ch-unload").is_none());
    }

    #[tokio::test]
    async fn unified_host_invoke_missing_channel() {
        let host = UnifiedWasmHost::new(WasmHostConfig::default()).unwrap();
        let result = host
            .invoke_hook(
                "no-such",
                HookKind::ChatRequest,
                Bytes::from_static(b"{}"),
                HookContext::default(),
            )
            .await
            .unwrap();
        assert!(result.is_none());
    }
}
