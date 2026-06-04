//! native:windsurf —— Windsurf Cascade 渠道。ADR-0005 重渠道，**PoC 骨架（gRPC 未接通）**。
//!
//! ## 为什么是 native，且为什么暂未接通
//!
//! windsurf 是 native plane 里最重的一档——manifest / WASM 全无可能：
//! 1. **fork/exec 本地二进制**：启动 `language_server_linux_x64`（路径由 `WINDSURF_LS_PATH` 或
//!    `/opt/windsurf/...` 等定位），父进程会 fork 出监听子进程后自退，需后台收尸防 zombie。
//! 2. **HTTP/2 明文 gRPC + 手写 Protobuf**：连本地 `127.0.0.1:42100`，调
//!    `exa.language_server_pb.LanguageServerService`，帧 = `[0][len:4BE][protobuf]`。
//! 3. **Cascade 会话状态机**：`StartCascade → SendUserCascadeMessage → 250ms 轮询
//!    GetCascadeTrajectorySteps/Trajectory` 直到 idle，再取 GeneratorMetadata 拿 token。
//!
//! 移植自 foxnio `providerimpl/windsurf`（14 源文件 / 3705 行）。完整 Rust 版需 `h2` gRPC +
//! `tokio::process` + 手写 proto codec，约 2000+ 行，且**依赖本机存在 LS 二进制**——离线无法
//! 真跑。故本 PoC 仅落：注册元数据 + 可单测的纯协议逻辑（protobuf varint wire / model 归一化 /
//! 回退式 tool_call 提取），`chat` 路径 **fail-loud** 明示未接通。gRPC 接通见 ADR-0005 收口表。

use super::{NativeBuildContext, NativeProviderRegistration};
use crate::capabilities::ProviderCapabilities;
use crate::error::ProviderError;
use std::sync::Arc;

pub(super) fn registration() -> NativeProviderRegistration {
    NativeProviderRegistration {
        name: "windsurf",
        capabilities: ProviderCapabilities {
            chat: true,
            streaming: true,
            tools: true,
            vision: true,
            ..ProviderCapabilities::none()
        },
        // PoC：fail-loud。windsurf 需 fork 本地 LS + HTTP/2 gRPC，尚未接通——构造即明确拒绝，
        // 不静默假装可用（对标 ADR-0005 fail-loud 原则）。gRPC 落地后换成真 provider。
        factory: Arc::new(|_ctx: &NativeBuildContext<'_>| {
            Err(ProviderError::Config(
                "native:windsurf 尚未接通：需 fork 本地 language_server 二进制 + HTTP/2 gRPC \
                 (Cascade 会话状态机)，PoC 阶段仅落纯协议逻辑。详见 ADR-0005 收口表 #7。"
                    .to_string(),
            ))
        }),
    }
}

// ── protobuf wire 纯逻辑（待 gRPC 接通启用，先单测锁正确性）─────────────
//
// foxnio windsurf 手写 protobuf 编解码（不走 prost 代码生成），核心是 LEB128 varint
// 与 `tag = (field_number << 3) | wire_type`。这里把基元先实现并单测，gRPC 帧组装复用。

/// protobuf wire type。
#[allow(dead_code)]
mod wire_type {
    pub const VARINT: u64 = 0;
    pub const LEN: u64 = 2;
}

/// LEB128 varint 编码。
#[allow(dead_code)]
fn encode_varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
    out
}

/// LEB128 varint 解码；从 `pos` 读取并前移，截断/溢出返回 None。
#[allow(dead_code)]
fn decode_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    while *pos < buf.len() {
        let byte = buf[*pos];
        *pos += 1;
        if shift >= 64 {
            return None; // 溢出
        }
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
    }
    None // 截断
}

/// 组装 `tag = (field_number << 3) | wire_type`。
#[allow(dead_code)]
fn proto_tag(field_number: u64, wire: u64) -> u64 {
    (field_number << 3) | wire
}

/// gRPC 帧封装：`[compression_flag:1=0][length:4BE][payload]`。
#[allow(dead_code)]
fn grpc_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(0); // 未压缩
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

// ── model 归一化 + 回退式 tool_call 提取（纯逻辑）──────────────────

/// 把用户 model 归一化成 Cascade alias 查找键：lower、`.`→`-`、剥常见前缀、压缩重复 `-`。
#[allow(dead_code)]
fn normalize_model_key(model: &str) -> String {
    let lowered = model.trim().to_ascii_lowercase().replace('.', "-");
    let stripped = lowered
        .strip_prefix("anthropic-")
        .or_else(|| lowered.strip_prefix("anthropic."))
        .unwrap_or(&lowered);
    // 压缩连续 '-'
    let mut out = String::with_capacity(stripped.len());
    let mut prev_dash = false;
    for c in stripped.chars() {
        if c == '-' {
            if !prev_dash {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

/// 回退模式：从文本里抠 `<tool_call>\n{"name":..,"arguments":..}\n</tool_call>` 块。
/// foxnio 旧模型无 native tool 时走此路。返回 (name, arguments_json) 列表。
#[allow(dead_code)]
fn extract_tool_calls(text: &str) -> Vec<(String, String)> {
    const OPEN: &str = "<tool_call>";
    const CLOSE: &str = "</tool_call>";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else { break };
        let block = after[..end].trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(block) {
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
            if !name.is_empty() {
                let args = v
                    .get("arguments")
                    .map(|a| {
                        a.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| a.to_string())
                    })
                    .unwrap_or_default();
                out.push((name.to_string(), args));
            }
        }
        rest = &after[end + CLOSE.len()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{NativeBuildContext, build_native_provider};

    #[test]
    fn registered_but_factory_fail_loud() {
        let names = crate::native::native_provider_names();
        assert!(names.contains(&"windsurf".to_string()), "got: {names:?}");
        // capabilities 自报（路由 matrix 可感知）
        let caps = crate::native::native_provider_capabilities("windsurf").expect("caps");
        assert!(caps.chat && caps.streaming);

        // 构造即 fail-loud，错误信息点明未接通原因
        let channel = make_channel();
        let ctx = NativeBuildContext {
            channel: &channel,
            secrets: std::collections::HashMap::new(),
            opts: crate::ProviderOpts::default(),
        };
        let err = build_native_provider("windsurf", &ctx)
            .err()
            .expect("should fail-loud");
        let msg = err.to_string();
        assert!(msg.contains("尚未接通"), "got: {msg}");
        assert!(msg.contains("gRPC"), "got: {msg}");
    }

    #[test]
    fn varint_roundtrip() {
        for v in [0u64, 1, 127, 128, 300, 16_384, u32::MAX as u64, u64::MAX] {
            let buf = encode_varint(v);
            let mut pos = 0;
            assert_eq!(decode_varint(&buf, &mut pos), Some(v), "v={v}");
            assert_eq!(pos, buf.len(), "consumed all bytes for v={v}");
        }
        // 已知编码：300 = 0xAC 0x02
        assert_eq!(encode_varint(300), vec![0xAC, 0x02]);
    }

    #[test]
    fn decode_varint_truncated_is_none() {
        // 0x80 标记后续还有字节，但 buf 截断 → None
        let buf = [0x80u8];
        let mut pos = 0;
        assert_eq!(decode_varint(&buf, &mut pos), None);
    }

    #[test]
    fn proto_tag_and_grpc_frame() {
        // field 1, LEN(2) → (1<<3)|2 = 10
        assert_eq!(proto_tag(1, wire_type::LEN), 10);
        // field 35, VARINT(0) → 35<<3 = 280
        assert_eq!(proto_tag(35, wire_type::VARINT), 280);

        let frame = grpc_frame(b"abc");
        assert_eq!(frame[0], 0); // 未压缩
        assert_eq!(&frame[1..5], &[0, 0, 0, 3]); // len BE
        assert_eq!(&frame[5..], b"abc");
    }

    #[test]
    fn normalize_model_key_variants() {
        assert_eq!(
            normalize_model_key("claude-3.5-sonnet"),
            "claude-3-5-sonnet"
        );
        assert_eq!(
            normalize_model_key("Anthropic.Claude-Sonnet-4-6"),
            "claude-sonnet-4-6"
        );
        assert_eq!(normalize_model_key("  claude--opus  "), "claude-opus");
    }

    #[test]
    fn extract_tool_calls_fallback() {
        let text = "blah <tool_call>\n{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\"}\n</tool_call> tail \
                    <tool_call>{\"name\":\"ls\",\"arguments\":{\"dir\":\".\"}}</tool_call>";
        let calls = extract_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "read_file");
        assert_eq!(calls[0].1, "{\"path\":\"a\"}");
        assert_eq!(calls[1].0, "ls");
        // 对象 arguments 序列化回 JSON 字符串
        assert!(calls[1].1.contains("\"dir\""));
    }

    #[test]
    fn extract_tool_calls_unterminated_safe() {
        assert!(extract_tool_calls("<tool_call>{\"name\":\"x\"}").is_empty());
        assert!(extract_tool_calls("no tags here").is_empty());
    }

    fn make_channel() -> gate_storage::ChannelRecord {
        let now = chrono::Utc::now();
        gate_storage::ChannelRecord {
            channel_id: gate_core::id::ChannelId::new(),
            code: "windsurf-test".into(),
            name: "windsurf-test".into(),
            provider_type: "native:windsurf".into(),
            base_url: String::new(),
            supported_models: vec![],
            status: "active".into(),
            health: "healthy".into(),
            timeout_ms: 60_000,
            max_retries: 2,
            rpm_limit: None,
            tpm_limit: None,
            tags: vec![],
            model_mapping: serde_json::json!({}),
            balance: None,
            balance_updated_at: None,
            last_error: None,
            last_error_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}
