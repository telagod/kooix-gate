//! gate-crypto: envelope encryption + KMS 抽象
//!
//! 设计：
//! - 每条密文用一把全新随机 DEK (32B AES-256-GCM key) 加密
//! - DEK 通过 KMS trait wrap 后与密文一同存储
//! - 主密钥（KEK）永不直接接触业务数据，便于轮换
//! - AAD 绑定上下文（如 channel_id），防止密文移植攻击
//!
//! 二进制格式：
//!   [1B version=1][12B wrap_nonce][48B wrapped_dek][12B data_nonce][N+16B ciphertext+tag]
//! 固定 89B 头部 + 明文长度。

pub mod aad;
pub mod envelope;
pub mod error;
pub mod kms;

pub use envelope::{HEADER_LEN, Sealer, VERSION};
pub use error::{CryptoError, Result};
pub use kms::{EnvKms, Kms};

/// 生产默认的 envelope sealer —— 基于 `EnvKms`（本地 master key）。
///
/// AppState 持有的是这个具体类型，gate-server 解密 `client_secret_enc` 等场景直接
/// `Arc<EnvelopeKms>::open(...)`。生产接 AWS/Vault 时把 `EnvKms` 换成对应 Kms
/// 并更新别名即可。
pub type EnvelopeKms = Sealer<EnvKms>;
