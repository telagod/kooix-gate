//! AAD 助手：把「业务上下文绑到密文」标准化
//!
//! 每条加密数据应用对应的 AAD，确保密文不能被搬到其他位置复用。
//! AAD 不需要保密，但解密时必须**字节相同**。
//!
//! 约定格式：`<domain>:<uuid_bytes>`，固定长度便于审计。
//!
//! 用法：
//! ```ignore
//! let aad = aad::channel_key(channel_key_id);
//! sealer.seal(plaintext_key, &aad).await?;
//! ```

use uuid::Uuid;

/// 通用构造器（内部用）
fn make(domain: &[u8], id: Uuid) -> Vec<u8> {
    let mut v = Vec::with_capacity(domain.len() + 1 + 16);
    v.extend_from_slice(domain);
    v.push(b':');
    v.extend_from_slice(id.as_bytes());
    v
}

/// Channel 配置 (channel.config_enc) 的 AAD
pub fn channel_config(channel_id: Uuid) -> Vec<u8> {
    make(b"channel_config", channel_id)
}

/// Channel Key (channel_keys.key_enc) 的 AAD
pub fn channel_key(channel_key_id: Uuid) -> Vec<u8> {
    make(b"channel_key", channel_key_id)
}

/// Identity Provider 的 client_secret 的 AAD
pub fn idp_secret(provider_id: Uuid) -> Vec<u8> {
    make(b"idp_secret", provider_id)
}

/// 通用：自定义 domain（仅在不便扩 API 时用）
pub fn custom(domain: &'static [u8], id: Uuid) -> Vec<u8> {
    debug_assert!(!domain.is_empty() && !domain.contains(&b':'));
    make(domain, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_domains_produce_different_aad() {
        let id = Uuid::now_v7();
        assert_ne!(channel_key(id), channel_config(id));
        assert_ne!(channel_key(id), idp_secret(id));
    }

    #[test]
    fn different_ids_produce_different_aad() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        assert_ne!(channel_key(a), channel_key(b));
    }

    #[test]
    fn stable_format() {
        let id = Uuid::from_u128(0x12345678_1234_1234_1234_123456789abc);
        let aad = channel_key(id);
        assert_eq!(&aad[..12], b"channel_key:");
        // "channel_key" (11) + ":" (1) + uuid (16) = 28
        assert_eq!(aad.len(), 28);
    }
}
