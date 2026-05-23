//! WASM transform 实战示例：在请求 body 中追加一个 system prompt。
//!
//! 用法：
//! ```bash
//! cd examples/wasm-transform
//! cargo build --target wasm32-unknown-unknown --release
//! sha256sum ../../target/wasm32-unknown-unknown/release/wasm_transform_example.wasm
//! ```
//!
//! 然后在 channel manifest 里挂：
//! ```json
//! {
//!   "plugin": {
//!     "version": 1,
//!     "preset": { "provider": "openai_compatible" },
//!     "security": {
//!       "wasm": {
//!         "module": "modules/wasm_transform_example.wasm",
//!         "module_sha256": "<sha256-hex>",
//!         "max_memory_bytes": 16777216,
//!         "max_cpu_ms": 50,
//!         "hooks": ["chat_request_transform"]
//!       }
//!     }
//!   }
//! }
//! ```

use gate_wasm_sdk::export_chat_request;
use serde_json::Value;

const SYSTEM_PROMPT: &str = "You are a careful assistant. Always cite sources.";

export_chat_request!(|body: &[u8]| -> Vec<u8> {
    // Parse JSON
    let mut req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return body.to_vec(), // identity fallback
    };

    // 在 messages 头部插一个 system prompt
    if let Some(messages) = req.get_mut("messages").and_then(|m| m.as_array_mut()) {
        // 检查是否已有 system message
        let has_system = messages
            .iter()
            .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"));
        if !has_system {
            messages.insert(
                0,
                serde_json::json!({
                    "role": "system",
                    "content": SYSTEM_PROMPT
                }),
            );
        }
    }

    serde_json::to_vec(&req).unwrap_or_else(|_| body.to_vec())
});
