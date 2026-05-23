//! gate-wasm: ADR-0003 WASM Plugin runtime（v0）。
//!
//! 提供 wasmtime 引擎托管的受限 transform runtime，挂载到 plugin manifest 的
//! request/response/stream hook 链路。
//!
//! # 设计约束（v0 hard limits）
//! - CPU: ≤ 50ms wall clock per call（fuel-based）
//! - Memory: ≤ 16 MiB linear memory
//! - No I/O: 禁 fs / net / clock 系统调用
//! - Deterministic: 禁 random，host 注入 monotonic clock 用于 metric
//!
//! # 当前状态
//!
//! v0 PoC：crate skeleton + WasmHost trait 接口设计完成，runtime 实现按版本
//! 推进（0.4.22-0.4.26）。

mod error;
pub mod fallback;
pub mod host;
pub mod limits;
pub mod wasmtime_host;

pub use error::{WasmError, WasmResult};
pub use fallback::invoke_with_fallback;
pub use host::{HookContext, HookKind, WasmHost, WasmHostConfig};
pub use limits::{DEFAULT_LIMITS, ResourceLimits};
pub use wasmtime_host::WasmtimeHost;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_defaults_match_adr0003() {
        let limits = DEFAULT_LIMITS;
        assert_eq!(limits.max_memory_bytes, 16 * 1024 * 1024);
        assert_eq!(limits.max_cpu_ms, 50);
    }

    #[test]
    fn host_config_default_is_safe() {
        let cfg = WasmHostConfig::default();
        assert!(cfg.limits.max_memory_bytes > 0);
        assert!(cfg.limits.max_cpu_ms > 0);
        assert!(!cfg.allow_fs);
        assert!(!cfg.allow_net);
    }

    #[test]
    fn hook_kind_str_roundtrip() {
        assert_eq!(HookKind::ChatRequest.as_str(), "chat_request_transform");
        assert_eq!(HookKind::ChatResponse.as_str(), "chat_response_transform");
        assert_eq!(HookKind::StreamChunk.as_str(), "stream_chunk_transform");
    }
}
