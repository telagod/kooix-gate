//! 控制台 JWT (HS256)
//!
//! 设计：
//! - 短 TTL access token（15 min）走 Authorization header
//! - 长 TTL refresh token（30 day）哈希入库，HttpOnly cookie 携带
//! - refresh 时滚动签发新对，旧 refresh 失效
//! - JWT 里只放 user_id + session_id + 当前 org，权限/角色每次实时查库

use crate::error::{AuthError, Result};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Secret 最小长度（字节）。低于此长度构造 `JwtIssuer` 会失败。
pub const MIN_SECRET_BYTES: usize = 32;
/// 推荐长度（生成器使用）
pub const RECOMMENDED_SECRET_BYTES: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: Uuid, // user_id
    pub sid: Uuid, // session_id
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    /// 当前激活的 org 上下文（控制台切换租户）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub org: Option<Uuid>,
    #[serde(default)]
    pub pa: bool, // platform admin shortcut
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshClaims {
    pub sub: Uuid,
    pub sid: Uuid,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    /// 防重放：refresh token 唯一 ID，对应 user_sessions.refresh_token_hash
    pub jti: Uuid,
}

#[derive(Clone)]
pub struct JwtIssuer {
    encoding: EncodingKey,
    decoding: DecodingKey,
    issuer: String,
    audience: String,
    access_ttl: Duration,
    refresh_ttl: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct TokenLifetimes {
    pub access: Duration,
    pub refresh: Duration,
}

impl Default for TokenLifetimes {
    fn default() -> Self {
        Self {
            access: Duration::minutes(15),
            refresh: Duration::days(30),
        }
    }
}

impl JwtIssuer {
    /// 构造 issuer。`secret` 必须 >= 32 字节，否则返回 `AuthError::Invalid`。
    pub fn new(
        secret: &[u8],
        issuer: impl Into<String>,
        audience: impl Into<String>,
        ttl: TokenLifetimes,
    ) -> Result<Self> {
        if secret.len() < MIN_SECRET_BYTES {
            return Err(AuthError::Invalid(format!(
                "JWT secret too short: {} bytes (minimum {})",
                secret.len(),
                MIN_SECRET_BYTES
            )));
        }
        Ok(Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            issuer: issuer.into(),
            audience: audience.into(),
            access_ttl: ttl.access,
            refresh_ttl: ttl.refresh,
        })
    }

    /// 从环境变量读 base64 编码的 secret 构造。
    pub fn from_env(
        var: &str,
        issuer: impl Into<String>,
        audience: impl Into<String>,
        ttl: TokenLifetimes,
    ) -> Result<Self> {
        let raw =
            std::env::var(var).map_err(|_| AuthError::Invalid(format!("env {var} missing")))?;
        let bytes = B64
            .decode(raw.trim())
            .map_err(|e| AuthError::Invalid(format!("env {var} not base64: {e}")))?;
        Self::new(&bytes, issuer, audience, ttl)
    }

    pub fn issue_access(
        &self,
        user_id: Uuid,
        session_id: Uuid,
        current_org: Option<Uuid>,
        is_platform_admin: bool,
    ) -> Result<(String, DateTime<Utc>)> {
        let now = Utc::now();
        let exp = now + self.access_ttl;
        let claims = AccessClaims {
            sub: user_id,
            sid: session_id,
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            org: current_org,
            pa: is_platform_admin,
        };
        let tok = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)?;
        Ok((tok, exp))
    }

    pub fn issue_refresh(
        &self,
        user_id: Uuid,
        session_id: Uuid,
        jti: Uuid,
    ) -> Result<(String, DateTime<Utc>)> {
        let now = Utc::now();
        let exp = now + self.refresh_ttl;
        let claims = RefreshClaims {
            sub: user_id,
            sid: session_id,
            iss: self.issuer.clone(),
            aud: format!("{}#refresh", self.audience),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            jti,
        };
        let tok = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)?;
        Ok((tok, exp))
    }

    pub fn parse_access(&self, token: &str) -> Result<AccessClaims> {
        let mut v = Validation::new(Algorithm::HS256);
        v.set_issuer(&[&self.issuer]);
        v.set_audience(&[&self.audience]);
        v.leeway = 5;
        let data =
            decode::<AccessClaims>(token, &self.decoding, &v).map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::TokenInvalid(e.to_string()),
            })?;
        Ok(data.claims)
    }

    pub fn parse_refresh(&self, token: &str) -> Result<RefreshClaims> {
        let mut v = Validation::new(Algorithm::HS256);
        v.set_issuer(&[&self.issuer]);
        v.set_audience(&[&format!("{}#refresh", self.audience)]);
        v.leeway = 5;
        let data =
            decode::<RefreshClaims>(token, &self.decoding, &v).map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::TokenInvalid(e.to_string()),
            })?;
        Ok(data.claims)
    }
}

/// 生成 64 字节随机 secret，base64 编码返回。
///
/// 部署时把这个值写到 `KOOIX_JWT_SECRET` 环境变量，永久保存。
/// 轮换会让所有现有会话失效。
pub fn generate_secret_b64() -> String {
    let mut buf = [0u8; RECOMMENDED_SECRET_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    B64.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuer() -> JwtIssuer {
        // 32 字节 ASCII 测试用 secret
        JwtIssuer::new(
            b"test-secret-32-bytes-minimum-ok!",
            "kg",
            "console",
            Default::default(),
        )
        .expect("32 bytes ok")
    }

    #[test]
    fn short_secret_rejected() {
        let r = JwtIssuer::new(b"too-short", "kg", "console", Default::default());
        assert!(matches!(r, Err(AuthError::Invalid(_))));
    }

    #[test]
    fn roundtrip() {
        let j = issuer();
        let uid = Uuid::now_v7();
        let sid = Uuid::now_v7();
        let (tok, _) = j.issue_access(uid, sid, None, false).unwrap();
        let claims = j.parse_access(&tok).unwrap();
        assert_eq!(claims.sub, uid);
        assert_eq!(claims.sid, sid);
    }

    #[test]
    fn refresh_aud_isolated() {
        let j = issuer();
        let uid = Uuid::now_v7();
        let sid = Uuid::now_v7();
        let (refresh, _) = j.issue_refresh(uid, sid, Uuid::now_v7()).unwrap();
        // refresh token 不应能当作 access 通过
        assert!(j.parse_access(&refresh).is_err());
    }

    #[test]
    fn generated_secret_is_valid_length() {
        let b64 = generate_secret_b64();
        let bytes = B64.decode(b64).unwrap();
        assert_eq!(bytes.len(), RECOMMENDED_SECRET_BYTES);
        assert!(JwtIssuer::new(&bytes, "kg", "c", Default::default()).is_ok());
    }
}
