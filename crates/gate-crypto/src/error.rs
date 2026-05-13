use thiserror::Error;

pub type Result<T> = std::result::Result<T, CryptoError>;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid key length: expected {expected}, got {got}")]
    InvalidKeyLength { expected: usize, got: usize },

    #[error("invalid ciphertext: {0}")]
    InvalidCiphertext(&'static str),

    #[error("unsupported envelope version: {0}")]
    UnsupportedVersion(u8),

    #[error("AEAD failed (tampering or wrong key/aad)")]
    AeadFailed,

    #[error("KMS error: {0}")]
    Kms(String),

    #[error("master key not configured: {0}")]
    MasterKeyMissing(String),

    #[error("decode error: {0}")]
    Decode(String),
}
