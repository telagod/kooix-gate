//! WASM runtime 错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WasmError {
    #[error("wasm module load failed: {0}")]
    Load(String),

    #[error("wasm module instantiate failed: {0}")]
    Instantiate(String),

    #[error("wasm hook {hook} call failed: {message}")]
    Call { hook: &'static str, message: String },

    #[error("wasm fuel exhausted (>{limit_ms}ms)")]
    Timeout { limit_ms: u64 },

    #[error("wasm linear memory limit exceeded (>{limit_bytes} bytes)")]
    OutOfMemory { limit_bytes: usize },

    #[error("wasm module sha256 mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },

    #[error("wasm host function denied: {0}")]
    HostDenied(String),

    #[error("wasm panic: {0}")]
    Panic(String),
}

pub type WasmResult<T> = Result<T, WasmError>;
