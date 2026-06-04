//! Example v1 WASM transform plugin for Kooix Gate.
//!
//! Prepends "[transformed] " to the model field in the JSON request,
//! proving the full WIT → guest SDK → component → host chain works.

wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/gate-wasm/wit/kooix-plugin.wit",
});

use exports::kooix::plugin::transform::Guest;
use kooix::plugin::types::{TransformError, TransformInput, TransformOutput};

struct PrefixPlugin;

impl Guest for PrefixPlugin {
    fn transform_request(input: TransformInput) -> Result<TransformOutput, TransformError> {
        let mut json: serde_json::Value = serde_json::from_str(&input.json)
            .map_err(|e| TransformError::InvalidInput(format!("bad json: {e}")))?;

        if let Some(model) = json.get("model").and_then(|v| v.as_str()).map(String::from) {
            json["model"] = serde_json::Value::String(format!("[transformed] {model}"));
        }

        Ok(TransformOutput {
            json: serde_json::to_string(&json).unwrap(),
            metadata: String::new(),
        })
    }

    fn transform_response(input: TransformInput) -> Result<TransformOutput, TransformError> {
        Ok(TransformOutput {
            json: input.json,
            metadata: "response-passthrough".into(),
        })
    }

    fn transform_stream_event(input: TransformInput) -> Result<TransformOutput, TransformError> {
        Ok(TransformOutput {
            json: input.json,
            metadata: String::new(),
        })
    }

    fn finish_stream(input: TransformInput) -> Result<TransformOutput, TransformError> {
        Ok(TransformOutput {
            json: input.json,
            metadata: "stream-finished".into(),
        })
    }
}

export!(PrefixPlugin);
