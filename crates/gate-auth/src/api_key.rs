//! API Key: 调用方凭证
//!
//! 设计：
//! - 明文格式：`sk-kg-<base64url(32B 随机数据)>`，共约 50 字符
//! - 数据库存 SHA-256 哈希 + prefix + last4，明文只在生成时返回一次
//! - 校验时 hash 后 constant-time 比较

use crate::error::{AuthError, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const PREFIX: &str = "sk-kg-";
pub const RANDOM_BYTES: usize = 32;
pub const PREFIX_DISPLAY_LEN: usize = 8;
pub const LAST4_LEN: usize = 4;

pub struct GeneratedKey {
    /// 完整明文，仅生成时返回一次
    pub plaintext: Zeroizing<String>,
    /// 'sk-kg-XXXXXXXX' 前缀，列表展示
    pub prefix: String,
    /// 末 4 位
    pub last4: String,
    /// 数据库存储用
    pub hash: String,
}

pub fn generate() -> GeneratedKey {
    let mut buf = [0u8; RANDOM_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    let body = B64URL.encode(buf);
    let plain = format!("{PREFIX}{body}");

    let prefix = plain[..PREFIX.len() + PREFIX_DISPLAY_LEN].to_string();
    let last4 = plain[plain.len() - LAST4_LEN..].to_string();
    let hash = hash(&plain);

    GeneratedKey {
        plaintext: Zeroizing::new(plain),
        prefix,
        last4,
        hash,
    }
}

pub fn hash(plaintext: &str) -> String {
    let mut h = Sha256::new();
    h.update(plaintext.as_bytes());
    hex::encode(h.finalize())
}

/// Constant-time 校验 plaintext 是否对应给定 hash。
///
/// 仅做格式 + 哈希校验，不查数据库——调用方先用 `hash(plaintext)`
/// 查库拿到候选 record，再用本函数确认。
pub fn verify(plaintext: &str, expected_hash_hex: &str) -> Result<()> {
    if !plaintext.starts_with(PREFIX) {
        return Err(AuthError::InvalidCredentials);
    }
    let actual = hash(plaintext);
    let a = actual.as_bytes();
    let b = expected_hash_hex.as_bytes();
    if a.len() != b.len() {
        return Err(AuthError::InvalidCredentials);
    }
    if a.ct_eq(b).into() {
        Ok(())
    } else {
        Err(AuthError::InvalidCredentials)
    }
}

/// 从 Authorization header 值中抽取 API key 明文。
/// 接受 `Bearer sk-kg-...` 或裸 `sk-kg-...`。
pub fn extract_from_header(value: &str) -> Option<&str> {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
        if rest.starts_with(PREFIX) {
            return Some(rest);
        }
    }
    if v.starts_with(PREFIX) {
        return Some(v);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_is_consistent() {
        let k = generate();
        assert!(k.plaintext.starts_with(PREFIX));
        assert_eq!(hash(&k.plaintext), k.hash);
        assert_eq!(k.last4, k.plaintext[k.plaintext.len() - 4..]);
        assert_eq!(k.prefix.len(), PREFIX.len() + PREFIX_DISPLAY_LEN);
        verify(&k.plaintext, &k.hash).unwrap();
    }

    #[test]
    fn wrong_plaintext_rejected() {
        let k = generate();
        assert!(verify("sk-kg-deadbeef", &k.hash).is_err());
        assert!(verify("not-a-key", &k.hash).is_err());
    }

    #[test]
    fn extracts_bearer() {
        let k = generate();
        let bearer = format!("Bearer {}", &*k.plaintext);
        assert_eq!(extract_from_header(&bearer), Some(k.plaintext.as_str()));
        assert_eq!(extract_from_header(&k.plaintext), Some(k.plaintext.as_str()));
        assert_eq!(extract_from_header("Bearer something-else"), None);
    }
}
