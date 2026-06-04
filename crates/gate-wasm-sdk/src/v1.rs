//! ADR-0006 v1 component-model guest SDK.
//!
//! Uses `wit-bindgen::generate!` to produce typed bindings from the
//! `kooix:plugin@0.1.0` WIT definition. Plugin authors implement the
//! `Guest` trait and use the `export_plugin!` macro.
//!
//! # Example
//!
//! ```rust,ignore
//! use gate_wasm_sdk::v1::*;
//!
//! struct MyPlugin;
//!
//! impl Guest for MyPlugin {
//!     fn transform_request(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput {
//!             json: input.json,
//!             metadata: String::new(),
//!         })
//!     }
//!
//!     fn transform_response(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//!
//!     fn transform_stream_event(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//!
//!     fn finish_stream(input: TransformInput) -> Result<TransformOutput, TransformError> {
//!         Ok(TransformOutput { json: input.json, metadata: String::new() })
//!     }
//! }
//!
//! export_plugin!(MyPlugin);
//! ```
//!
//! # Building
//!
//! ```bash
//! cargo build --target wasm32-wasip1 --release --features v1
//! # Then componentize:
//! wasm-tools component new target/wasm32-wasip1/release/my_plugin.wasm -o my_plugin.component.wasm
//! ```

wit_bindgen::generate!({
    world: "plugin",
    path: "../gate-wasm/wit/kooix-plugin.wit",
    pub_export_macro: true,
    default_bindings_module: "gate_wasm_sdk::v1",
});

pub use exports::kooix::plugin::transform::Guest;
pub use kooix::plugin::host::{
    get_secret, log, nonce, now_ms, record_metric, redact, SecretBytes, SecretError, SecretRef,
};
pub use kooix::plugin::types::{TransformError, TransformInput, TransformOutput};
