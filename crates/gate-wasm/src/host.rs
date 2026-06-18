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
    /// 0.4.83（product-gaps G-104）：cwasm 编译产物缓存目录。
    /// `None` = 禁用缓存，每次 load 都重新 compile（旧行为）。
    /// `Some(path)` = 启动优先 `Module::deserialize_file`，失败 fallback 到 compile +
    /// 写回 `{path}/{sha256}-{wasmtime_version}.cwasm`。
    pub cache_dir: Option<std::path::PathBuf>,
}

impl Default for WasmHostConfig {
    fn default() -> Self {
        Self {
            limits: ResourceLimits::default(),
            allow_fs: false,
            allow_net: false,
            deterministic: true,
            cache_dir: None,
        }
    }
}

impl WasmHostConfig {
    /// 0.4.84：从 env 读 `KOOIX_WASM_CACHE_DIR` 注入 cache_dir。
    /// 空字符串或未设 → None（不启用 cwasm 缓存）。
    /// 设置后即 `Some(PathBuf)`；不做 dir-exists 检查（首次 load 会自动 create_dir_all）。
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(dir) = std::env::var("KOOIX_WASM_CACHE_DIR") {
            if !dir.is_empty() {
                cfg.cache_dir = Some(std::path::PathBuf::from(dir));
            }
        }
        cfg
    }
}

/// Hook 调用上下文。
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub channel_id: String,
    pub model: String,
    pub request_id: String,
    pub metadata: HashMap<String, String>,
    /// 0.4.136（按 docs/backlog/wasm-secret-slot-design.md）：secret 解密发生在调用方，
    /// 这里 host 拿到的已经是按 manifest `security.permissions.secret_slots`
    /// 过滤好的明文 map。`allowed_slots` 同步声明合法 slot 名（防 caller 忘传
    /// 时所有 slot 都拒绝）。
    pub secrets: HashMap<String, String>,
    pub allowed_slots: std::collections::HashSet<String>,
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
