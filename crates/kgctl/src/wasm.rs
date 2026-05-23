//! `kgctl wasm` — ADR-0003 v0 WASM 模块工具
//!
//! 子命令：
//!   kgctl wasm verify <path>    打印 sha256 + 文件大小
//!   kgctl wasm inspect <path>   读取 wasm 模块导出 + 检查 ABI v0 必要 export

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub fn verify(path: PathBuf) -> Result<()> {
    let bytes = std::fs::read(&path)
        .with_context(|| format!("读取 wasm 模块失败: {}", path.display()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    let sha = hex::encode(h.finalize());
    println!("path:    {}", path.display());
    println!("size:    {} bytes", bytes.len());
    println!("sha256:  {}", sha);
    println!("\n复制到 channel manifest:");
    println!("  \"security\": {{");
    println!("    \"wasm\": {{");
    println!("      \"module\": \"{}\",", path.file_name().and_then(|n| n.to_str()).unwrap_or("module.wasm"));
    println!("      \"module_sha256\": \"{}\",", sha);
    println!("      \"max_memory_bytes\": 16777216,");
    println!("      \"max_cpu_ms\": 50,");
    println!("      \"hooks\": [\"chat_request_transform\"]");
    println!("    }}");
    println!("  }}");
    Ok(())
}

pub fn inspect(path: PathBuf) -> Result<()> {
    let bytes = std::fs::read(&path)
        .with_context(|| format!("读取 wasm 模块失败: {}", path.display()))?;

    // 用 wasmparser 看 export
    let mut found_alloc = false;
    let mut found_memory = false;
    let mut hook_exports: Vec<&'static str> = Vec::new();
    const REQUIRED_HOOKS: &[&str] = &[
        "chat_request_transform",
        "chat_response_transform",
        "stream_chunk_transform",
    ];

    use wasmparser::{Parser, Payload};
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let export = export?;
                let name = export.name;
                if name == "gate_alloc" {
                    found_alloc = true;
                }
                if name == "memory" {
                    found_memory = true;
                }
                for hook in REQUIRED_HOOKS {
                    if &name == hook {
                        hook_exports.push(hook);
                    }
                }
            }
        }
    }

    println!("path:                 {}", path.display());
    println!("size:                 {} bytes", bytes.len());
    println!("export `memory`:      {}", if found_memory { "✓" } else { "✗" });
    println!("export `gate_alloc`:  {}", if found_alloc { "✓" } else { "✗" });
    println!("hooks:                {}", if hook_exports.is_empty() { "(none — identity passthrough)".to_string() } else { hook_exports.join(", ") });

    if !found_memory || !found_alloc {
        anyhow::bail!("wasm 模块缺少 ABI v0 必需 export (memory / gate_alloc)");
    }
    if hook_exports.is_empty() {
        eprintln!("\n警告：未导出任何 hook，runtime 将走 identity passthrough");
    }
    Ok(())
}
