//! WasmHost trait — 0.4.22 落地具体 wasmtime 实现，先把接口锁住。

use crate::error::WasmResult;
use crate::limits::ResourceLimits;
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;

/// ADR-0003 v0 三个 hook。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookKind {
    ChatRequest,
    ChatResponse,
    StreamChunk,
}

impl HookKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookKind::ChatRequest => "chat_request_transform",
            HookKind::ChatResponse => "chat_response_transform",
            HookKind::StreamChunk => "stream_chunk_transform",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WasmHostConfig {
    pub limits: ResourceLimits,
    pub allow_fs: bool,
    pub allow_net: bool,
    pub deterministic: bool,
}

impl Default for WasmHostConfig {
    fn default() -> Self {
        Self {
            limits: ResourceLimits::default(),
            allow_fs: false,
            allow_net: false,
            deterministic: true,
        }
    }
}

/// Hook 调用上下文。
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub channel_id: String,
    pub model: String,
    pub request_id: String,
    pub metadata: HashMap<String, String>,
}

/// WasmHost trait — runtime 实现需提供 module 加载 + hook 调用。
#[async_trait]
pub trait WasmHost: Send + Sync {
    /// 加载 wasm 模块；module_bytes 必须匹配 expected_sha256。
    async fn load_module(
        &self,
        channel_id: &str,
        module_bytes: &[u8],
        expected_sha256: &str,
    ) -> WasmResult<()>;

    /// 调用某个 hook。返回值：transform 后的 payload；None 表示模块未实现该 hook。
    async fn invoke_hook(
        &self,
        channel_id: &str,
        hook: HookKind,
        payload: Bytes,
        ctx: HookContext,
    ) -> WasmResult<Option<Bytes>>;

    /// 卸载模块（channel 删除时调用）。
    async fn unload_module(&self, channel_id: &str) -> WasmResult<()>;
}
