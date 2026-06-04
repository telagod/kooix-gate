//! Native provider plane —— ADR-0005 命令式渠道层。
//!
//! ## 为什么存在
//!
//! 声明式 manifest 层（[`crate::custom_provider::CustomHttpProvider`]）覆盖"长得
//! 像标准 HTTP API"的上游：差异只在 URL / body 模板 / 响应字段路径 / 鉴权策略，
//! 这些都能用 JSONB manifest 声明，运行期解释执行、零代码热加载。
//!
//! 但有一类"重渠道"（kiro / windsurf 这种逆向私有产品 API 的上游）需要**图灵
//! 完备的过程逻辑**——启动本地二进制、手写 gRPC/Protobuf、会话状态机、干扰检测
//! 重试、token 预估。这些 manifest 永远表达不了。本模块给它们留一条命令式逃生口：
//! 每个重渠道是一个实现 [`Provider`] trait 的 Rust 模块，编译进二进制，按名字注册。
//!
//! ## 寻址约定
//!
//! `channel.provider_type = "native:<name>"`，路由层 strip `native:` 前缀后查注册表。
//! 这也为"`/<渠道名>` 显式寻址、少一层 group 路由"留好了入口（见 ADR-0005）。
//!
//! ## 新增一个 native 渠道
//!
//! 1. 写 `native/<name>.rs`，实现 [`Provider`]，暴露 `pub(super) fn registration()`；
//! 2. 在 [`builtin_registrations`] 里加一行 `v.push(<name>::registration());`
//!    （编译期静态注册，类似 foxnio 的 `providerplugins/all.go`）。
//!
//! 外部 crate / 未来动态加载可走 [`register_native_provider`] 运行期注册。

use crate::Provider;
use crate::capabilities::ProviderCapabilities;
use crate::error::{ProviderError, ProviderResult};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

mod codex;
#[cfg(test)]
mod echo;
mod kiro;
mod windsurf;

/// `provider_type` 的 native 命名空间前缀。
pub const NATIVE_PREFIX: &str = "native:";

/// native provider 构造上下文。
///
/// 路由层在选中 channel、解析完 secret slots 后，把这些交给 factory；native
/// 实现自行决定如何使用（kiro 取 `primary` token，windsurf 解析 ProfileArn 等）。
pub struct NativeBuildContext<'a> {
    pub channel: &'a gate_storage::ChannelRecord,
    /// 解密后的 secret slots（label → plaintext），至少含 `primary`。
    pub secrets: HashMap<String, String>,
    pub opts: crate::ProviderOpts,
}

impl NativeBuildContext<'_> {
    /// 取某个 secret slot；缺失返回空串（native 实现自行决定是否 fail）。
    pub fn secret(&self, slot: &str) -> &str {
        self.secrets.get(slot).map(String::as_str).unwrap_or("")
    }

    /// primary slot 便捷取值。
    pub fn primary_secret(&self) -> &str {
        self.secret("primary")
    }
}

/// factory 签名：从上下文构造一个 `Arc<dyn Provider>`。
pub type NativeProviderFactory =
    Arc<dyn Fn(&NativeBuildContext<'_>) -> ProviderResult<Arc<dyn Provider>> + Send + Sync>;

/// 一个 native provider 的注册元数据 + 工厂。
///
/// `capabilities` 让路由层的 capability matrix 能感知 native 渠道能力，无需在
/// 路由代码里写 `if provider == "kiro"`（对标 foxnio 的 `Descriptor.Capabilities`）。
#[derive(Clone)]
pub struct NativeProviderRegistration {
    pub name: &'static str,
    pub capabilities: ProviderCapabilities,
    pub factory: NativeProviderFactory,
}

fn registry() -> &'static RwLock<HashMap<String, NativeProviderRegistration>> {
    static REG: OnceLock<RwLock<HashMap<String, NativeProviderRegistration>>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut map = HashMap::new();
        for reg in builtin_registrations() {
            map.insert(reg.name.to_string(), reg);
        }
        RwLock::new(map)
    })
}

/// 编译期静态注册的内置 native 渠道。新增重渠道在这里加一行。
fn builtin_registrations() -> Vec<NativeProviderRegistration> {
    #[allow(unused_mut)]
    let mut v = vec![
        kiro::registration(),
        codex::registration(),
        windsurf::registration(),
    ];
    #[cfg(test)]
    v.push(echo::registration());
    v
}

/// 判断 `provider_type` 是否落在 native 命名空间。
pub fn is_native_provider_type(provider_type: &str) -> bool {
    provider_type.starts_with(NATIVE_PREFIX)
}

/// 从 `native:<name>` 取 `<name>`（trim 后）；非 native 前缀返回 None。
pub fn native_name(provider_type: &str) -> Option<&str> {
    provider_type
        .strip_prefix(NATIVE_PREFIX)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 运行期注册一个 native provider（外部 crate / 测试 / 未来动态加载入口）。
///
/// 同名覆盖。注册表全局共享，进程级生效。
pub fn register_native_provider(reg: NativeProviderRegistration) {
    registry().write().insert(reg.name.to_string(), reg);
}

/// 查某 native provider 声明的 capabilities。
pub fn native_provider_capabilities(name: &str) -> Option<ProviderCapabilities> {
    registry().read().get(name).map(|r| r.capabilities.clone())
}

/// 当前已注册的 native provider 名字（排序，用于诊断 / admin 展示）。
pub fn native_provider_names() -> Vec<String> {
    let mut v: Vec<String> = registry().read().keys().cloned().collect();
    v.sort();
    v
}

/// 按名字构造 native provider 实例。
pub fn build_native_provider(
    name: &str,
    ctx: &NativeBuildContext<'_>,
) -> ProviderResult<Arc<dyn Provider>> {
    // 先 clone factory 出锁，避免 factory 内部再访问注册表造成重入死锁。
    let factory = registry().read().get(name).map(|r| r.factory.clone());
    match factory {
        Some(factory) => factory(ctx),
        None => Err(ProviderError::Config(format!(
            "no native provider registered for '{name}' (provider_type='{NATIVE_PREFIX}{name}'); \
             registered native providers: [{}]",
            native_provider_names().join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, ChatRequest, Role};

    fn dummy_channel(provider_type: &str) -> gate_storage::ChannelRecord {
        let now = chrono::Utc::now();
        gate_storage::ChannelRecord {
            channel_id: gate_core::id::ChannelId::new(),
            code: "test-native".into(),
            name: "test-native".into(),
            provider_type: provider_type.into(),
            base_url: "https://unused.example".into(),
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

    #[test]
    fn native_name_strips_prefix() {
        assert_eq!(native_name("native:kiro"), Some("kiro"));
        assert_eq!(native_name("native: echo "), Some("echo"));
        assert_eq!(native_name("native:"), None);
        assert_eq!(native_name("plugin"), None);
        assert!(is_native_provider_type("native:kiro"));
        assert!(!is_native_provider_type("plugin"));
    }

    #[test]
    fn echo_is_registered_and_advertises_chat() {
        let names = native_provider_names();
        assert!(names.contains(&"echo".to_string()), "got: {names:?}");
        let caps = native_provider_capabilities("echo").expect("echo caps");
        assert!(caps.chat);
        assert!(caps.streaming);
    }

    #[tokio::test]
    async fn echo_roundtrips_chat() {
        let channel = dummy_channel("native:echo");
        let ctx = NativeBuildContext {
            channel: &channel,
            secrets: HashMap::new(),
            opts: crate::ProviderOpts::default(),
        };
        let provider = build_native_provider("echo", &ctx).expect("build echo");
        assert_eq!(provider.name(), "native:echo");

        let req = ChatRequest {
            model: "echo-1".to_string(),
            messages: vec![ChatMessage::text(Role::User, "ping")],
            ..Default::default()
        };
        let resp = provider.chat(req).await.expect("echo chat");
        assert_eq!(resp.choices[0].message.content_text(), "ping");
    }

    #[test]
    fn unknown_native_provider_is_fail_loud() {
        let channel = dummy_channel("native:does-not-exist");
        let ctx = NativeBuildContext {
            channel: &channel,
            secrets: HashMap::new(),
            opts: crate::ProviderOpts::default(),
        };
        let result = build_native_provider("does-not-exist", &ctx);
        assert!(result.is_err());
        let msg = result.err().expect("should be err").to_string();
        assert!(msg.contains("no native provider registered"), "got: {msg}");
    }
}
