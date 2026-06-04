//! ADR-0006 v1 component-model guest SDK reference.
//!
//! Plugin authors generate their own bindings directly from the WIT file using
//! `wit_bindgen::generate!`. This module serves as documentation and type
//! re-exports for host-side code that needs to reference the generated types.
//!
//! # Plugin Usage (guest side)
//!
//! In your plugin crate's `lib.rs`:
//!
//! ```rust,ignore
//! wit_bindgen::generate!({
//!     world: "plugin",
//!     path: "path/to/kooix-plugin.wit",  // from crates/gate-wasm/wit/
//! });
//!
//! use exports::kooix::plugin::transform::Guest;
//! use kooix::plugin::types::{TransformError, TransformInput, TransformOutput};
//!
//! struct MyPlugin;
//!
//! impl Guest for MyPlugin {
//!     fn transform_request(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//!     fn transform_response(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//!     fn transform_stream_event(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//!     fn finish_stream(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//! }
//!
//! export!(MyPlugin);
//! ```
//!
//! # Build & Componentize
//!
//! ```bash
//! cargo build --target wasm32-unknown-unknown --release
//! wasm-tools component new target/wasm32-unknown-unknown/release/my_plugin.wasm \
//!   -o my_plugin.component.wasm
//! ```
//!
//! # Host Functions (available inside transform hooks)
//!
//! ```rust,ignore
//! // from kooix::plugin::host (imported by the generated bindings)
//! let secret = get_secret(SecretRef { slot: "primary".into(), purpose: "auth".into() })?;
//! log(2, "transform applied");
//! record_metric("custom_counter", 1.0);
//! let ts = now_ms();
//! let random = nonce(16);
//! let safe = redact(sensitive_value);
//! ```

// Re-export the WIT file path for reference.
pub const WIT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../gate-wasm/wit/kooix-plugin.wit");
