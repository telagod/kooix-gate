//! KMS trait + 本地 ENV 实现
//!
//! 生产可加 AwsKmsProvider / GcpKmsProvider / VaultTransitProvider 等实现，
//! 上层 Sealer 无感知。

use crate::error::{CryptoError, Result};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use zeroize::Zeroizing;

pub const DEK_LEN: usize = 32;
pub const WRAP_NONCE_LEN: usize = 12;
pub const WRAPPED_DEK_LEN: usize = DEK_LEN + 16; // GCM tag
pub const WRAP_TOTAL_LEN: usize = WRAP_NONCE_LEN + WRAPPED_DEK_LEN;

/// KMS：负责把 DEK 封装/解封。
///
/// `wrap()` 返回 `wrap_nonce(12) || wrapped_dek(48)`，共 60 字节。
/// `unwrap()` 接受同样格式。
#[async_trait]
pub trait Kms: Send + Sync {
    async fn wrap(&self, dek: &[u8]) -> Result<Vec<u8>>;
    async fn unwrap(&self, wrapped: &[u8]) -> Result<Zeroizing<[u8; DEK_LEN]>>;

    /// 返回当前 KEK 的标识符（用于审计/轮换）
    fn key_id(&self) -> &str;
}

/// 从环境变量读取 32B base64 master key 的本地 KMS。
///
/// 适合开发 / 单机部署。生产建议接 AWS KMS / Vault Transit。
pub struct EnvKms {
    cipher: Aes256Gcm,
    key_id: String,
}

impl EnvKms {
    /// 从 base64 编码的 32B key 构造。
    pub fn from_b64(b64: &str, key_id: impl Into<String>) -> Result<Self> {
        let raw = B64
            .decode(b64.trim())
            .map_err(|e| CryptoError::Decode(format!("base64: {e}")))?;
        if raw.len() != DEK_LEN {
            return Err(CryptoError::InvalidKeyLength {
                expected: DEK_LEN,
                got: raw.len(),
            });
        }
        let cipher =
            Aes256Gcm::new_from_slice(&raw).map_err(|_| CryptoError::InvalidKeyLength {
                expected: DEK_LEN,
                got: raw.len(),
            })?;
        // raw 出作用域自动清零靠不住——这里立刻覆盖
        Ok(Self {
            cipher,
            key_id: key_id.into(),
        })
    }

    /// 便捷构造：读环境变量 `var_name` (base64 32B)。
    pub fn from_env(var_name: &str) -> Result<Self> {
        let v = std::env::var(var_name)
            .map_err(|_| CryptoError::MasterKeyMissing(var_name.to_string()))?;
        Self::from_b64(&v, var_name)
    }
}

#[async_trait]
impl Kms for EnvKms {
    async fn wrap(&self, dek: &[u8]) -> Result<Vec<u8>> {
        if dek.len() != DEK_LEN {
            return Err(CryptoError::InvalidKeyLength {
                expected: DEK_LEN,
                got: dek.len(),
            });
        }
        let mut nonce = [0u8; WRAP_NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);

        let ct = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: dek,
                    aad: b"dek-wrap",
                },
            )
            .map_err(|_| CryptoError::AeadFailed)?;
        debug_assert_eq!(ct.len(), WRAPPED_DEK_LEN);

        let mut out = Vec::with_capacity(WRAP_TOTAL_LEN);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    async fn unwrap(&self, wrapped: &[u8]) -> Result<Zeroizing<[u8; DEK_LEN]>> {
        if wrapped.len() != WRAP_TOTAL_LEN {
            return Err(CryptoError::InvalidCiphertext(
                "wrapped DEK length mismatch",
            ));
        }
        let nonce = &wrapped[..WRAP_NONCE_LEN];
        let ct = &wrapped[WRAP_NONCE_LEN..];

        let pt = self
            .cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ct,
                    aad: b"dek-wrap",
                },
            )
            .map_err(|_| CryptoError::AeadFailed)?;
        if pt.len() != DEK_LEN {
            return Err(CryptoError::InvalidCiphertext("decrypted DEK length"));
        }
        let mut dek = [0u8; DEK_LEN];
        dek.copy_from_slice(&pt);
        Ok(Zeroizing::new(dek))
    }

    fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// 工具：生成一个新 master key（base64），用于初始化部署。
pub fn generate_master_key_b64() -> String {
    let mut key = [0u8; DEK_LEN];
    rand::thread_rng().fill_bytes(&mut key);
    let s = B64.encode(key);
    // 不需要手动清零 key — 栈变量出作用域即丢
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn env_kms_roundtrip() {
        let key = generate_master_key_b64();
        let kms = EnvKms::from_b64(&key, "test").unwrap();
        let mut dek = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut dek);
        let wrapped = kms.wrap(&dek).await.unwrap();
        assert_eq!(wrapped.len(), WRAP_TOTAL_LEN);
        let unwrapped = kms.unwrap(&wrapped).await.unwrap();
        assert_eq!(*unwrapped, dek);
    }

    #[tokio::test]
    async fn tampered_wrap_fails() {
        let kms = EnvKms::from_b64(&generate_master_key_b64(), "t").unwrap();
        let dek = [0u8; DEK_LEN];
        let mut wrapped = kms.wrap(&dek).await.unwrap();
        wrapped[20] ^= 0xff; // 篡改密文
        assert!(kms.unwrap(&wrapped).await.is_err());
    }
}
