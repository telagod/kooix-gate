//! AWS SigV4 + HMAC primitives，对 crate 内复用。
//!
//! 同时被 `bedrock.rs`（编译期 fast-path provider）与 `custom_provider`
//! （manifest runtime auth strategy = AwsSigv4 / Hmac）使用。
//!
//! 业务面的高层 sign_request 入口在各 caller 自己实现：bedrock.rs 是
//! `BedrockProvider::sign_request`，custom_provider 是 `apply_auth_headers` 的
//! `AuthStrategy::AwsSigv4` 分支。这里只提供底层运算 + canonicalization。

use crate::error::{ProviderError, ProviderResult};
use hmac::{Hmac, Mac};
use reqwest::Url;
use sha2::{Digest, Sha256};

pub(crate) type HmacSha256 = Hmac<Sha256>;

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn hmac_sha256(key: &[u8], msg: &[u8]) -> ProviderResult<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| ProviderError::Config(format!("invalid hmac key: {e}")))?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().to_vec())
}

pub(crate) fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> ProviderResult<String> {
    hmac_sha256(key, msg).map(hex::encode)
}

pub(crate) fn aws_sigv4_signing_key(
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

pub(crate) fn infer_aws_region_from_host(host: &str) -> Option<String> {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() >= 4
        && labels[0].starts_with("bedrock-runtime")
        && labels.last().is_some_and(|tld| *tld == "com")
    {
        return Some(labels[1].to_string());
    }
    None
}

pub(crate) fn canonical_uri(url: &Url) -> String {
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

pub(crate) fn canonical_query_string(url: &Url) -> String {
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

pub(crate) fn uri_encode(value: &str) -> String {
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
