//! OIDC 客户端封装
//!
//! 设计：
//! - 一个 `OidcProvider` 对应数据库 `identity_providers` 一行
//! - `start()` 返回授权 URL + 服务端需要保存的 PKCE/nonce/state
//! - `exchange()` 用回调 code 换取 id_token，验证 nonce，提取用户 claims
//! - state 的持久化在 server 层（落 `oidc_login_states` 表）
//!
//! HTTP 层使用复用的 reqwest client，并显式禁用 redirect，避免 discovery / token exchange 被重定向放大成 SSRF 面。

use crate::error::{AuthError, Result};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointSet, IssuerUrl,
    Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use url::Url;

/// 一次「开始登录」的产物，**必须服务端保存**直到回调到来。
#[derive(Debug)]
pub struct OidcStart {
    pub authorize_url: Url,
    pub pkce_verifier: PkceCodeVerifier,
    pub nonce: Nonce,
    pub csrf_state: CsrfToken,
}

/// 从 OIDC userinfo / id_token 中提取的标准化身份。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcIdentity {
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub raw_claims: serde_json::Value,
}

/// 单个身份提供者的运行时句柄。
///
/// 构造代价较高（需要 issuer discovery），建议按 provider_id 缓存。
pub struct OidcProvider {
    pub provider_id: uuid::Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    client: CoreClient<
        EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        EndpointSet,
        EndpointMaybeSet,
    >,
    http_client: reqwest::Client,
}

impl OidcProvider {
    /// 通过 OIDC discovery 端点初始化客户端。
    ///
    /// `redirect_uri` 是 callback URL，必须与 IdP 控制台配置一致。
    pub async fn discover(
        provider_id: uuid::Uuid,
        name: String,
        issuer: &str,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
        scopes: Vec<String>,
    ) -> Result<Self> {
        let issuer_url =
            IssuerUrl::new(issuer.to_string()).map_err(|e| AuthError::Oidc(e.to_string()))?;
        let redirect_url = RedirectUrl::new(redirect_uri.to_string())
            .map_err(|e| AuthError::Oidc(format!("redirect: {e}")))?;

        let http_client = oidc_http_client()?;

        let metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
            .await
            .map_err(|e| AuthError::Oidc(format!("discover: {e}")))?;
        let token_url = metadata
            .token_endpoint()
            .cloned()
            .ok_or_else(|| AuthError::Oidc("issuer metadata missing token_endpoint".into()))?;

        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(client_id.to_string()),
            Some(ClientSecret::new(client_secret.to_string())),
        )
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url);

        Ok(Self {
            provider_id,
            name,
            scopes,
            client,
            http_client,
        })
    }

    /// 生成授权 URL 与对应的服务端状态。
    pub fn start(&self) -> OidcStart {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let mut auth = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(pkce_challenge);

        for s in &self.scopes {
            auth = auth.add_scope(Scope::new(s.clone()));
        }

        let (url, csrf_state, nonce) = auth.url();
        OidcStart {
            authorize_url: url,
            pkce_verifier,
            nonce,
            csrf_state,
        }
    }

    /// 回调阶段：用授权码换 token + 校验 nonce，返回标准化身份。
    pub async fn exchange(
        &self,
        code: &str,
        pkce_verifier: PkceCodeVerifier,
        expected_nonce: &Nonce,
    ) -> Result<OidcIdentity> {
        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&self.http_client)
            .await
            .map_err(|e| AuthError::Oidc(format!("token exchange: {e}")))?;

        // 校验 ID token
        let id_token = token_response
            .id_token()
            .ok_or_else(|| AuthError::Oidc("missing id_token".into()))?;

        let id_token_verifier = self.client.id_token_verifier();
        let claims = id_token
            .claims(&id_token_verifier, expected_nonce)
            .map_err(|e| AuthError::Oidc(format!("id_token claims: {e}")))?;

        let subject = claims.subject().to_string();
        let email = claims.email().map(|e| e.as_str().to_string());
        let email_verified = claims.email_verified().unwrap_or(false);
        let name = claims
            .name()
            .and_then(|n| n.get(None))
            .map(|n| n.as_str().to_string());

        let raw_claims = serde_json::to_value(claims).unwrap_or(serde_json::Value::Null);

        Ok(OidcIdentity {
            subject,
            email,
            email_verified,
            name,
            raw_claims,
        })
    }

    /// 暴露 csrf state secret，用于持久化与回调比较
    pub fn csrf_secret(state: &CsrfToken) -> &str {
        state.secret()
    }
}

fn oidc_http_client() -> Result<reqwest::Client> {
    reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AuthError::Oidc(format!("http client: {e}")))
}
