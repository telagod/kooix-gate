use anyhow::{Context, bail};
use std::fs;
use std::io::{self, Read};

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

fn read_manifest(path: Option<String>) -> anyhow::Result<String> {
    match path.as_deref() {
        Some("-") | None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("read manifest from stdin")?;
            if buf.trim().is_empty() {
                bail!("plugin manifest input is empty");
            }
            Ok(buf)
        }
        Some(path) => fs::read_to_string(path).with_context(|| format!("read {path}")),
    }
}
