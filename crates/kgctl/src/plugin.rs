use anyhow::{Context, bail};
use gate_providers::types::{ChatMessage, ChatRequest, Role};
use gate_providers::{CustomHttpProvider, Provider};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginFixture {
    version: u8,
    base_url: String,
    model: String,
    manifest: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_sample: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_sse: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_chunks: Option<Vec<gate_providers::types::ChatStreamChunk>>,
}

pub fn schema() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&gate_providers::plugin_manifest_schema_json())?
    );
    Ok(())
}

pub fn lint(path: Option<String>, base_url: String) -> anyhow::Result<()> {
    let input = read_manifest(path)?;
    let value: serde_json::Value =
        serde_json::from_str(&input).context("plugin manifest must be valid JSON")?;
    gate_providers::validate_plugin_manifest(value, &base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("plugin manifest ok");
    Ok(())
}

pub fn replay(
    manifest_path: Option<String>,
    sse_path: String,
    base_url: String,
    model: String,
) -> anyhow::Result<()> {
    let input = read_manifest(manifest_path)?;
    let value: serde_json::Value =
        serde_json::from_str(&input).context("plugin manifest must be valid JSON")?;
    let raw_sse = fs::read(&sse_path).with_context(|| format!("read {sse_path}"))?;
    let chunks = gate_providers::replay_plugin_sse(value, &base_url, raw_sse, &model)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("{}", serde_json::to_string_pretty(&chunks)?);
    Ok(())
}

pub async fn test_connection(
    manifest_path: Option<String>,
    base_url: String,
    api_key: String,
    model: String,
    prompt: String,
    max_tokens: u32,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let value = parse_manifest(read_manifest(manifest_path)?)?;
    let provider = CustomHttpProvider::new_with_opts(
        &base_url,
        api_key,
        value,
        gate_providers::ProviderOpts { timeout_ms },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let req = ChatRequest {
        model,
        messages: vec![ChatMessage::text(Role::User, prompt)],
        max_tokens: Some(max_tokens),
        temperature: Some(0.0),
        stream: false,
        ..Default::default()
    };
    let response = provider
        .chat(req)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

pub fn export_fixture(
    manifest_path: Option<String>,
    sse_path: Option<String>,
    response_sample_path: Option<String>,
    output: Option<String>,
    base_url: String,
    model: String,
) -> anyhow::Result<()> {
    let manifest = parse_manifest(read_manifest(manifest_path)?)?;
    gate_providers::validate_plugin_manifest(manifest.clone(), &base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let response_sample = response_sample_path
        .as_deref()
        .map(read_json_file)
        .transpose()?;
    let raw_sse = sse_path
        .as_deref()
        .map(|path| fs::read_to_string(path).with_context(|| format!("read {path}")))
        .transpose()?;
    let expected_chunks = raw_sse
        .as_ref()
        .map(|raw| {
            gate_providers::replay_plugin_sse(manifest.clone(), &base_url, raw.as_bytes(), &model)
                .map_err(|e| anyhow::anyhow!(e.to_string()))
        })
        .transpose()?;

    let fixture = PluginFixture {
        version: 1,
        base_url,
        model,
        manifest,
        response_sample,
        raw_sse,
        expected_chunks,
    };
    write_output(output, &serde_json::to_string_pretty(&fixture)?)?;
    Ok(())
}

pub fn import_fixture(
    fixture_path: Option<String>,
    verify: bool,
    output: Option<String>,
) -> anyhow::Result<()> {
    let input = read_named_input(fixture_path, "fixture")?;
    let fixture: PluginFixture =
        serde_json::from_str(&input).context("plugin fixture must be valid JSON")?;
    if fixture.version != 1 {
        bail!("unsupported plugin fixture version: {}", fixture.version);
    }
    gate_providers::validate_plugin_manifest(fixture.manifest.clone(), &fixture.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    if verify {
        match (&fixture.raw_sse, &fixture.expected_chunks) {
            (Some(raw), Some(expected)) => {
                let actual = gate_providers::replay_plugin_sse(
                    fixture.manifest.clone(),
                    &fixture.base_url,
                    raw.as_bytes(),
                    &fixture.model,
                )
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let actual_json = normalize_replay_chunks(serde_json::to_value(&actual)?);
                let expected_json = normalize_replay_chunks(serde_json::to_value(expected)?);
                if actual_json != expected_json {
                    bail!(
                        "plugin fixture replay mismatch: expected {} chunks, got {} chunks",
                        expected.len(),
                        actual.len()
                    );
                }
            }
            (None, _) => bail!("fixture has no raw_sse to verify"),
            (_, None) => bail!("fixture has no expected_chunks to verify"),
        }
    }

    if output.is_some() {
        write_output(output, &serde_json::to_string_pretty(&fixture.manifest)?)?;
    } else {
        println!("plugin fixture ok");
    }
    Ok(())
}

fn parse_manifest(input: String) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(&input).context("plugin manifest must be valid JSON")
}

fn read_manifest(path: Option<String>) -> anyhow::Result<String> {
    read_named_input(path, "manifest")
}

fn read_named_input(path: Option<String>, kind: &str) -> anyhow::Result<String> {
    match path.as_deref() {
        Some("-") | None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .with_context(|| format!("read {kind} from stdin"))?;
            if buf.trim().is_empty() {
                bail!("plugin {kind} input is empty");
            }
            Ok(buf)
        }
        Some(path) => fs::read_to_string(path).with_context(|| format!("read {path}")),
    }
}

fn read_json_file(path: &str) -> anyhow::Result<serde_json::Value> {
    let input = fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    serde_json::from_str(&input).with_context(|| format!("{path} must be valid JSON"))
}

fn write_output(path: Option<String>, content: &str) -> anyhow::Result<()> {
    match path.as_deref() {
        Some("-") | None => {
            println!("{content}");
            Ok(())
        }
        Some(path) => fs::write(path, content).with_context(|| format!("write {path}")),
    }
}

fn normalize_replay_chunks(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(chunks) = value.as_array_mut() {
        for chunk in chunks {
            if let Some(obj) = chunk.as_object_mut()
                && obj
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id.starts_with("chatcmpl-"))
            {
                obj.insert(
                    "id".to_string(),
                    serde_json::Value::String("<generated-chatcmpl-id>".to_string()),
                );
            }
        }
    }
    value
}
