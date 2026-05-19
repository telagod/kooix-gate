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
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode,
    errors::ErrorKind as JwtErrorKind,
};
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
pub struct JwtRing {
    encoding: EncodingKey,
    decodings: Vec<DecodingKey>,
    issuer: String,
    audience: String,
    access_ttl: Duration,
    refresh_ttl: Duration,
}

/// Backward-compatible name used by the server: issues with the primary key and
/// verifies with the full ring.
pub type JwtIssuer = JwtRing;

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

impl JwtRing {
    /// 构造 JWT ring。`secret` 是 primary signing key，必须 >= 32 字节。
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
            decodings: vec![DecodingKey::from_secret(secret)],
            issuer: issuer.into(),
            audience: audience.into(),
            access_ttl: ttl.access,
            refresh_ttl: ttl.refresh,
        })
    }

    /// 从环境变量读 base64 编码的 primary secret 构造。
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

    /// 从 primary env + optional previous env 构造双密钥窗口。
    ///
    /// `previous_var` 支持逗号分隔多个 base64 secret。新 token 只用 primary
    /// 签发；解析 access/refresh 时会按 primary → previous 顺序验证。
    pub fn from_env_with_previous(
        primary_var: &str,
        previous_var: &str,
        issuer: impl Into<String>,
        audience: impl Into<String>,
        ttl: TokenLifetimes,
    ) -> Result<Self> {
        let issuer = issuer.into();
        let audience = audience.into();
        let mut ring = Self::from_env(primary_var, issuer, audience, ttl)?;
        let Ok(raw_previous) = std::env::var(previous_var) else {
            return Ok(ring);
        };
        for (idx, raw) in raw_previous
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .enumerate()
        {
            let bytes = B64.decode(raw).map_err(|e| {
                AuthError::Invalid(format!("env {previous_var}[{idx}] not base64: {e}"))
            })?;
            ring = ring.with_previous_secret(&bytes)?;
        }
        Ok(ring)
    }

    /// 追加旧 secret 到验证窗口。只用于 verify，不用于签发。
    pub fn with_previous_secret(mut self, secret: &[u8]) -> Result<Self> {
        if secret.len() < MIN_SECRET_BYTES {
            return Err(AuthError::Invalid(format!(
                "JWT previous secret too short: {} bytes (minimum {})",
                secret.len(),
                MIN_SECRET_BYTES
            )));
        }
        self.decodings.push(DecodingKey::from_secret(secret));
        Ok(self)
    }

    pub fn previous_secret_count(&self) -> usize {
        self.decodings.len().saturating_sub(1)
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
        decode_with_ring(token, &self.decodings, &v)
    }

    pub fn parse_refresh(&self, token: &str) -> Result<RefreshClaims> {
        let mut v = Validation::new(Algorithm::HS256);
        v.set_issuer(&[&self.issuer]);
        v.set_audience(&[&format!("{}#refresh", self.audience)]);
        v.leeway = 5;
        decode_with_ring(token, &self.decodings, &v)
    }
}

fn decode_with_ring<T>(token: &str, decodings: &[DecodingKey], validation: &Validation) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut expired_seen = false;
    let mut last_error = None;
    for decoding in decodings {
        match decode::<T>(token, decoding, validation) {
            Ok(data) => return Ok(data.claims),
            Err(e) => match e.kind() {
                JwtErrorKind::ExpiredSignature => {
                    expired_seen = true;
                    last_error = Some(e.to_string());
                }
                _ => last_error = Some(e.to_string()),
            },
        }
    }
    if expired_seen {
        Err(AuthError::TokenExpired)
    } else {
        Err(AuthError::TokenInvalid(
            last_error.unwrap_or_else(|| "no JWT verifier configured".into()),
        ))
    }
}

/// 生成 64 字节随机 secret，base64 编码返回。
///
/// 部署时把这个值写到 `KOOIX_JWT_SECRET` 环境变量。
/// 正常轮换：新值放 `KOOIX_JWT_SECRET`，旧值临时放 `KOOIX_JWT_PREVIOUS_SECRETS`。
pub fn generate_secret_b64() -> String {
    let mut buf = [0u8; RECOMMENDED_SECRET_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    B64.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    const PRIMARY_SECRET: &[u8] = b"primary-secret-32-bytes-minimum-ok";
    const PREVIOUS_SECRET: &[u8] = b"previous-secret-32-bytes-minimum!";
    const SECOND_PREVIOUS_SECRET: &[u8] = b"second-previous-secret-32-bytes!!";

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_env(key: &str, value: &str) {
        // SAFETY: env-mutating tests hold `env_lock`, so this crate does not
        // concurrently mutate/read the tested keys.
        unsafe { std::env::set_var(key, value) };
    }

    fn remove_env(key: &str) {
        // SAFETY: env-mutating tests hold `env_lock`, so this crate does not
        // concurrently mutate/read the tested keys.
        unsafe { std::env::remove_var(key) };
    }

    struct EnvGuard {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            Self {
                values: keys.iter().map(|&k| (k, std::env::var(k).ok())).collect(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(v) => set_env(key, v),
                    None => remove_env(key),
                }
            }
        }
    }

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

    #[test]
    fn ring_accepts_previous_secret_for_access_and_refresh() {
        let old = JwtIssuer::new(PREVIOUS_SECRET, "kg", "console", Default::default()).unwrap();
        let ring = JwtIssuer::new(PRIMARY_SECRET, "kg", "console", Default::default())
            .unwrap()
            .with_previous_secret(PREVIOUS_SECRET)
            .unwrap();

        assert_eq!(ring.previous_secret_count(), 1);

        let uid = Uuid::now_v7();
        let sid = Uuid::now_v7();
        let jti = Uuid::now_v7();

        let (old_access, _) = old.issue_access(uid, sid, None, false).unwrap();
        let (old_refresh, _) = old.issue_refresh(uid, sid, jti).unwrap();

        assert_eq!(ring.parse_access(&old_access).unwrap().sub, uid);
        assert_eq!(ring.parse_refresh(&old_refresh).unwrap().jti, jti);

        let (new_access, _) = ring.issue_access(uid, sid, None, false).unwrap();
        let (new_refresh, _) = ring.issue_refresh(uid, sid, Uuid::now_v7()).unwrap();

        assert!(old.parse_access(&new_access).is_err());
        assert!(old.parse_refresh(&new_refresh).is_err());
        assert_eq!(ring.parse_access(&new_access).unwrap().sub, uid);
        assert_eq!(ring.parse_refresh(&new_refresh).unwrap().sub, uid);
    }

    #[test]
    fn from_env_with_previous_accepts_comma_separated_previous() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::new(&["KOOIX_TEST_JWT_PRIMARY", "KOOIX_TEST_JWT_PREVIOUS"]);

        set_env("KOOIX_TEST_JWT_PRIMARY", &B64.encode(PRIMARY_SECRET));
        set_env(
            "KOOIX_TEST_JWT_PREVIOUS",
            &format!(
                " {}, {} ",
                B64.encode(PREVIOUS_SECRET),
                B64.encode(SECOND_PREVIOUS_SECRET)
            ),
        );

        let ring = JwtIssuer::from_env_with_previous(
            "KOOIX_TEST_JWT_PRIMARY",
            "KOOIX_TEST_JWT_PREVIOUS",
            "kg",
            "console",
            Default::default(),
        )
        .unwrap();

        assert_eq!(ring.previous_secret_count(), 2);

        let old =
            JwtIssuer::new(SECOND_PREVIOUS_SECRET, "kg", "console", Default::default()).unwrap();
        let uid = Uuid::now_v7();
        let sid = Uuid::now_v7();
        let (token, _) = old.issue_access(uid, sid, None, false).unwrap();
        assert_eq!(ring.parse_access(&token).unwrap().sid, sid);
    }

    #[test]
    fn short_previous_secret_rejected() {
        let result = JwtIssuer::new(PRIMARY_SECRET, "kg", "console", Default::default())
            .unwrap()
            .with_previous_secret(b"too-short");
        let Err(err) = result else {
            panic!("short previous secret must be rejected");
        };
        assert!(matches!(err, AuthError::Invalid(_)));
    }

    #[test]
    fn invalid_previous_base64_rejected() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::new(&["KOOIX_TEST_JWT_PRIMARY", "KOOIX_TEST_JWT_PREVIOUS"]);

        set_env("KOOIX_TEST_JWT_PRIMARY", &B64.encode(PRIMARY_SECRET));
        set_env("KOOIX_TEST_JWT_PREVIOUS", "not base64");

        let result = JwtIssuer::from_env_with_previous(
            "KOOIX_TEST_JWT_PRIMARY",
            "KOOIX_TEST_JWT_PREVIOUS",
            "kg",
            "console",
            Default::default(),
        );
        let Err(err) = result else {
            panic!("invalid previous secret base64 must be rejected");
        };
        assert!(matches!(err, AuthError::Invalid(_)));
    }

    #[test]
    fn short_previous_secret_from_env_rejected() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::new(&["KOOIX_TEST_JWT_PRIMARY", "KOOIX_TEST_JWT_PREVIOUS"]);

        set_env("KOOIX_TEST_JWT_PRIMARY", &B64.encode(PRIMARY_SECRET));
        set_env("KOOIX_TEST_JWT_PREVIOUS", &B64.encode(b"too-short"));

        let result = JwtIssuer::from_env_with_previous(
            "KOOIX_TEST_JWT_PRIMARY",
            "KOOIX_TEST_JWT_PREVIOUS",
            "kg",
            "console",
            Default::default(),
        );
        let Err(err) = result else {
            panic!("short previous secret from env must be rejected");
        };
        assert!(matches!(err, AuthError::Invalid(_)));
    }
}
