//! plugin_manifest 内部 JSON pointer / config error 辅助。

use crate::error::ProviderError;

pub(super) fn path_to_json_pointer(path: &serde_path_to_error::Path) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            serde_path_to_error::Segment::Seq { index } => {
                out.push('/');
                out.push_str(&index.to_string());
            }
            serde_path_to_error::Segment::Map { key }
            | serde_path_to_error::Segment::Enum { variant: key } => {
                out.push('/');
                out.push_str(&escape_json_pointer(key));
            }
            serde_path_to_error::Segment::Unknown => out.push_str("/?"),
        }
    }
    out
}

pub(super) fn json_pointer(base: &str, suffix: &str) -> String {
    match (base.is_empty(), suffix.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => suffix.to_string(),
        (false, true) => base.to_string(),
        (false, false) => format!("{base}{suffix}"),
    }
}

pub(super) fn escape_json_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

pub(super) fn config_at(base: &str, suffix: &str, message: &str) -> ProviderError {
    ProviderError::Config(format!(
        "invalid plugin manifest at {}: {message}",
        json_pointer(base, suffix)
    ))
}
