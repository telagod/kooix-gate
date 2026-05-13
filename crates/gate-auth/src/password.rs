//! Argon2id 密码哈希/校验
//!
//! 参数遵循 OWASP 2024 建议：m=64MiB, t=3, p=4。

use crate::error::{AuthError, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

fn argon2() -> Argon2<'static> {
    // m=64MiB (64*1024 KiB), t=3, p=4
    let params = Params::new(64 * 1024, 3, 4, None).expect("valid params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// 哈希密码。返回 PHC string 可直接存数据库。
pub fn hash(password: &str) -> Result<String> {
    if password.len() < 8 || password.len() > 1024 {
        return Err(AuthError::PasswordTooWeak);
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AuthError::Hash(e.to_string()))?;
    Ok(hash.to_string())
}

/// 校验密码。不匹配返回 `InvalidCredentials`，**不区分**密码错与用户不存在。
pub fn verify(password: &str, phc: &str) -> Result<()> {
    let parsed = PasswordHash::new(phc).map_err(|e| AuthError::Hash(e.to_string()))?;
    argon2()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AuthError::InvalidCredentials)
}

/// 检查 PHC 是否需要重哈希（参数升级时用）。
///
/// 简化策略：只要算法不是 argon2id 就认为需要升级。
/// 后续若调高 m/t/p 参数，可在此处加版本号比较。
pub fn needs_rehash(phc: &str) -> bool {
    PasswordHash::new(phc)
        .map(|p| p.algorithm.as_str() != "argon2id")
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let phc = hash("correct horse battery staple").unwrap();
        assert!(verify("correct horse battery staple", &phc).is_ok());
        assert!(verify("wrong", &phc).is_err());
    }

    #[test]
    fn too_short_rejected() {
        assert!(matches!(hash("short"), Err(AuthError::PasswordTooWeak)));
    }
}
