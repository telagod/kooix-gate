//! Envelope encryption: 上层 API
//!
//! 用法:
//! ```ignore
//! let kms = EnvKms::from_env("KOOIX_MASTER_KEY")?;
//! let sealer = Sealer::new(kms);
//! let ct = sealer.seal(b"my-api-key", b"channel:abc-123").await?;
//! // ... 存数据库 ...
//! let pt = sealer.open(&ct, b"channel:abc-123").await?;
//! ```

use crate::error::{CryptoError, Result};
use crate::kms::{DEK_LEN, Kms, WRAP_TOTAL_LEN};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::RngCore;
use zeroize::Zeroizing;

pub const VERSION: u8 = 1;
pub const DATA_NONCE_LEN: usize = 12;
pub const HEADER_LEN: usize = 1 + WRAP_TOTAL_LEN + DATA_NONCE_LEN;

/// 密封器。无状态——`K` 实现自带任何需要的状态。
pub struct Sealer<K: Kms> {
    kms: K,
}

impl<K: Kms> Sealer<K> {
    pub fn new(kms: K) -> Self {
        Self { kms }
    }

    pub fn kms(&self) -> &K {
        &self.kms
    }

    /// 加密。`aad` 绑定上下文（推荐填入资源 ID），防密文移植。
    pub async fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        // 1. 生成 DEK
        let mut dek = Zeroizing::new([0u8; DEK_LEN]);
        rand::thread_rng().fill_bytes(&mut *dek);

        // 2. 通过 KMS 封装 DEK
        let wrapped = self.kms.wrap(&*dek).await?;
        debug_assert_eq!(wrapped.len(), WRAP_TOTAL_LEN);

        // 3. 用 DEK 加密明文
        let cipher =
            Aes256Gcm::new_from_slice(&*dek).map_err(|_| CryptoError::InvalidKeyLength {
                expected: DEK_LEN,
                got: dek.len(),
            })?;
        let mut data_nonce = [0u8; DATA_NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut data_nonce);

        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&data_nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::AeadFailed)?;

        // 4. 拼装
        let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        out.push(VERSION);
        out.extend_from_slice(&wrapped);
        out.extend_from_slice(&data_nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// 解密。`aad` 必须与 seal 时一致，否则失败。
    pub async fn open(&self, sealed: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if sealed.len() < HEADER_LEN {
            return Err(CryptoError::InvalidCiphertext("too short"));
        }
        let version = sealed[0];
        if version != VERSION {
            return Err(CryptoError::UnsupportedVersion(version));
        }

        let wrapped = &sealed[1..1 + WRAP_TOTAL_LEN];
        let data_nonce = &sealed[1 + WRAP_TOTAL_LEN..HEADER_LEN];
        let ciphertext = &sealed[HEADER_LEN..];

        let dek = self.kms.unwrap(wrapped).await?;
        let cipher =
            Aes256Gcm::new_from_slice(&*dek).map_err(|_| CryptoError::InvalidKeyLength {
                expected: DEK_LEN,
                got: dek.len(),
            })?;

        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(data_nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::AeadFailed)?;
        Ok(Zeroizing::new(plaintext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kms::{EnvKms, generate_master_key_b64};

    #[tokio::test]
    async fn seal_open_roundtrip() {
        let kms = EnvKms::from_b64(&generate_master_key_b64(), "t").unwrap();
        let sealer = Sealer::new(kms);
        let pt = b"sk-openai-XXXXXXXXXXXXXXXXXX";
        let aad = b"channel:abc";
        let ct = sealer.seal(pt, aad).await.unwrap();
        assert!(ct.len() > HEADER_LEN);
        let opened = sealer.open(&ct, aad).await.unwrap();
        assert_eq!(&*opened, pt);
    }

    #[tokio::test]
    async fn wrong_aad_fails() {
        let kms = EnvKms::from_b64(&generate_master_key_b64(), "t").unwrap();
        let sealer = Sealer::new(kms);
        let ct = sealer.seal(b"secret", b"context-a").await.unwrap();
        assert!(sealer.open(&ct, b"context-b").await.is_err());
    }

    #[tokio::test]
    async fn tampered_ciphertext_fails() {
        let kms = EnvKms::from_b64(&generate_master_key_b64(), "t").unwrap();
        let sealer = Sealer::new(kms);
        let mut ct = sealer.seal(b"secret", b"ctx").await.unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0xff;
        assert!(sealer.open(&ct, b"ctx").await.is_err());
    }
}
