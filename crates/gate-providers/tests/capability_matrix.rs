//! Capability matrix golden test — ADR-0002 验收锚点。
//!
//! 锁定 4 个 fast-path provider 的 9 维 capability 与 base_url 默认值，让任何
//! manifest preset 漂移都被立刻拦下。M3 引入 `builtin_fastpath` 后，fast-path
//! 路径的 capability 必须与 manifest runtime 路径**字节级**一致，这个 fixture
//! 是合同测试。
//!
//! Fixture 路径：`crates/gate-providers/tests/fixtures/capability_matrix.json`
//!
//! 刷新方式：
//!
//! ```bash
//! KOOIX_UPDATE_FIXTURES=1 cargo test -p gate-providers --test capability_matrix
//! ```
//!
//! 触发刷新后，diff fixture 文件，确认变化是预期的，再 commit。

use gate_providers::{
    ProviderCapabilities, plugin_preset_base_url_suggestion, plugin_preset_capabilities,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// ADR-0002 锁定的 4 个 fast-path preset。新增 fast-path provider 时同步更新这里 +
/// fixture 文件 + ADR-0002 验收清单。
const FAST_PATH_PRESETS: &[&str] = &[
    "openai",
    "anthropic_messages",
    "azure_openai",
    "bedrock_converse",
];

/// 全部 23 个 preset 的 capability 也跟着锁，防止整个 plugin runtime 静默漂移。
/// 顺序与 `plugin_preset.rs` ProviderPresetKind 枚举一致。
const ALL_PRESETS: &[&str] = &[
    "openai",
    "openai_compatible",
    "deepseek",
    "mistral",
    "gemini",
    "azure_openai",
    "vertex_openai",
    "anthropic_messages",
    "bedrock_converse",
    "cohere_chat",
    "groq",
    "together",
    "openrouter",
    "moonshot",
    "zhipu",
    "qwen",
    "yi",
    "ollama",
    "vllm",
    "lm_studio",
    "ollama_openai",
    "localai",
    "xinference",
];

#[derive(Debug, Serialize, PartialEq, Eq)]
struct PresetSnapshot {
    capabilities: ProviderCapabilities,
    base_url_suggestion: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct CapabilityMatrix {
    fast_path: BTreeMap<String, PresetSnapshot>,
    all_presets: BTreeMap<String, PresetSnapshot>,
}

fn snapshot_for(preset: &str) -> PresetSnapshot {
    let capabilities = plugin_preset_capabilities(preset)
        .unwrap_or_else(|| panic!("preset '{preset}' not registered in plugin_preset.rs"));
    let base_url_suggestion = plugin_preset_base_url_suggestion(preset).map(str::to_string);
    PresetSnapshot {
        capabilities,
        base_url_suggestion,
    }
}

fn current_matrix() -> CapabilityMatrix {
    let mut fast_path = BTreeMap::new();
    for preset in FAST_PATH_PRESETS {
        fast_path.insert((*preset).to_string(), snapshot_for(preset));
    }
    let mut all_presets = BTreeMap::new();
    for preset in ALL_PRESETS {
        all_presets.insert((*preset).to_string(), snapshot_for(preset));
    }
    CapabilityMatrix {
        fast_path,
        all_presets,
    }
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("capability_matrix.json")
}

fn render(matrix: &CapabilityMatrix) -> String {
    let mut s = serde_json::to_string_pretty(matrix).expect("serialize matrix");
    s.push('\n');
    s
}

#[test]
fn capability_matrix_matches_golden() {
    let path = fixture_path();
    let actual = render(&current_matrix());

    if std::env::var_os("KOOIX_UPDATE_FIXTURES").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixtures dir");
        }
        fs::write(&path, &actual).expect("write fixture");
        println!("capability_matrix fixture refreshed at {}", path.display());
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "fixture missing at {}: {err}\n\
             首次运行请用 KOOIX_UPDATE_FIXTURES=1 cargo test -p gate-providers --test capability_matrix",
            path.display()
        )
    });

    if actual != expected {
        let diff_path = path.with_extension("json.actual");
        fs::write(&diff_path, &actual).ok();
        panic!(
            "capability matrix drifted from fixture.\n\
             expected: {}\n\
             actual  : {}\n\
             如果是预期变化，跑：KOOIX_UPDATE_FIXTURES=1 cargo test -p gate-providers --test capability_matrix",
            path.display(),
            diff_path.display()
        );
    }
}

#[test]
fn fast_path_presets_have_chat_streaming() {
    // ADR-0002 invariant: fast-path 4 个 provider 必须支持 chat + streaming，
    // 否则 fast-path 没意义（它的存在就是为了优化高 QPS chat/stream 路径）。
    for preset in FAST_PATH_PRESETS {
        let caps = plugin_preset_capabilities(preset)
            .unwrap_or_else(|| panic!("preset '{preset}' not registered"));
        assert!(caps.chat, "fast-path preset '{preset}' must support chat");
        assert!(
            caps.streaming,
            "fast-path preset '{preset}' must support streaming"
        );
    }
}

#[test]
fn all_registered_presets_appear_in_matrix() {
    // 防御：plugin_preset.rs 新增 ProviderPresetKind 时，必须把新名字加进
    // ALL_PRESETS。这个测试 + capability_matrix_matches_golden 双锁。
    for preset in ALL_PRESETS {
        assert!(
            plugin_preset_capabilities(preset).is_some(),
            "ALL_PRESETS contains '{preset}' but plugin_preset_capabilities returns None — \
             这意味着 ALL_PRESETS 包含了一个废弃的 preset 名字，请同步删除。"
        );
    }
}
