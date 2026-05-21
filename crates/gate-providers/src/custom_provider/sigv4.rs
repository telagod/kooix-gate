//! AWS SigV4 + HMAC primitives for plugin manifest auth strategies。
//!
//! 同时支持 `hmac` 与 `aws_sigv4` auth strategy 的底层签名运算。
//! `AwsSigv4Signature` struct 留在 mod.rs（与 CustomHttpProvider impl 紧耦合）。

use super::HmacSha256;
use crate::error::{ProviderError, ProviderResult};
use hmac::Mac;
use reqwest::Url;
use sha2::{Digest, Sha256};

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(super) fn hmac_sha256(key: &[u8], msg: &[u8]) -> ProviderResult<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| ProviderError::Config(format!("invalid hmac key: {e}")))?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().to_vec())
}

pub(super) fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> ProviderResult<String> {
    hmac_sha256(key, msg).map(hex::encode)
}

pub(super) fn aws_sigv4_signing_key(
    secret_key: &str,
    date: &str,
    region: &str,
    service: &str,
) -> ProviderResult<Vec<u8>> {
    let k_date = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes())?;
    let k_region = hmac_sha256(&k_date, region.as_bytes())?;
    let k_service = hmac_sha256(&k_region, service.as_bytes())?;
    hmac_sha256(&k_service, b"aws4_request")
}

pub(super) fn infer_aws_region_from_host(host: &str) -> Option<String> {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() >= 4
        && labels[0].starts_with("bedrock-runtime")
        && labels.last().is_some_and(|tld| *tld == "com")
    {
        return Some(labels[1].to_string());
    }
    None
}

pub(super) fn canonical_uri(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() {
        "/".to_string()
    } else {
        path.split('/')
            .map(uri_encode)
            .collect::<Vec<_>>()
            .join("/")
    }
}

pub(super) fn canonical_query_string(url: &Url) -> String {
    let Some(query) = url.query() else {
        return String::new();
    };
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (uri_encode(&k), uri_encode(&v)))
        .collect();
    pairs.sort();
    if pairs.is_empty() && !query.is_empty() {
        return query.to_string();
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

pub(super) fn uri_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
