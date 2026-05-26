//! /v1/admin/identity-providers — SSO / OIDC provider 管理。
//!
//! 0.4.126：从 admin/mod.rs 物理拆出（7 handler + 14 normalize/helper fn + 2 type，~590 行）。
//! 依赖 admin/mod.rs 顶层 require_confirmation / audit_meta helper。

use super::*;
#[allow(unused_imports)]
use super::channels::{require_confirmation, audit_meta, channel_audit_snapshot, key_audit_snapshot, group_audit_snapshot, pricing_rule_audit_snapshot, user_audit_snapshot, channel_capabilities, channel_inflight, is_plugin_provider, key_fingerprint, validate_channel_key_alias, record_to_summary};

// ============================================================================
// Identity Providers / SSO (Admin)
// ============================================================================

#[derive(Deserialize)]
pub struct IdentityProvidersQuery {
    #[serde(default = "super::users::default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Serialize)]
pub struct IdentityProviderView {
    pub id: String,
    pub org_id: Option<String>,
    pub name: String,
    pub slug: String,
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub email_claim: String,
    pub name_claim: String,
    pub subject_claim: String,
    pub auto_create_users: bool,
    pub auto_join_org_role: Option<String>,
    pub email_domain_allowlist: Vec<String>,
    pub enabled: bool,
    pub redirect_policy: RedirectPolicyView,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RedirectPolicyView {
    #[serde(default = "default_true")]
    pub allow_relative: bool,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

#[derive(Deserialize)]
pub struct CreateIdentityProviderRequest {
    pub name: String,
    pub slug: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub email_claim: Option<String>,
    #[serde(default)]
    pub name_claim: Option<String>,
    #[serde(default)]
    pub subject_claim: Option<String>,
    #[serde(default = "default_true")]
    pub auto_create_users: bool,
    #[serde(default)]
    pub auto_join_org_role: Option<String>,
    #[serde(default)]
    pub email_domain_allowlist: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub redirect_policy: Option<RedirectPolicyView>,
}

#[derive(Deserialize)]
pub struct UpdateIdentityProviderRequest {
    #[serde(default)]
    pub org_id: Option<Option<String>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub email_claim: Option<String>,
    #[serde(default)]
    pub name_claim: Option<String>,
    #[serde(default)]
    pub subject_claim: Option<String>,
    #[serde(default)]
    pub auto_create_users: Option<bool>,
    #[serde(default)]
    pub auto_join_org_role: Option<Option<String>>,
    #[serde(default)]
    pub email_domain_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub redirect_policy: Option<RedirectPolicyView>,
}

#[derive(Deserialize)]
pub struct DiscoverIdentityProviderRequest {
    pub issuer: String,
}

#[derive(Serialize)]
pub struct DiscoverIdentityProviderResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub scopes_supported: Vec<String>,
}

pub(super) async fn list_identity_providers(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<IdentityProvidersQuery>,
) -> AppResult<Json<Vec<IdentityProviderView>>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let providers = app
        .repos
        .identity_providers
        .list(q.limit.clamp(1, 200), q.offset.max(0))
        .await?;
    Ok(Json(
        providers
            .into_iter()
            .map(identity_provider_to_view)
            .collect(),
    ))
}

pub(super) async fn create_identity_provider(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<CreateIdentityProviderRequest>,
) -> AppResult<Json<IdentityProviderView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let id = Uuid::now_v7();
    let org_id = parse_optional_org_id(req.org_id.as_deref())?;
    let name = normalize_non_empty(req.name, "name")?;
    let slug = normalize_slug(&req.slug)?;
    let issuer = normalize_https_url(req.issuer, "issuer")?;
    let client_id = normalize_non_empty(req.client_id, "client_id")?;
    let scopes = normalize_scopes(Some(req.scopes));
    let email_claim = normalize_claim(req.email_claim, "email")?;
    let name_claim = normalize_claim(req.name_claim, "name")?;
    let subject_claim = normalize_claim(req.subject_claim, "sub")?;
    let auto_join_org_role = normalize_optional_org_role(req.auto_join_org_role)?;
    let email_domain_allowlist = normalize_domain_allowlist(Some(req.email_domain_allowlist))?;
    let redirect_policy = normalize_redirect_policy(req.redirect_policy.unwrap_or_default())?;
    let client_secret_enc = seal_idp_secret(&app, id, &req.client_secret).await?;

    let provider = app
        .repos
        .identity_providers
        .create(IdentityProviderCreate {
            id,
            org_id,
            name,
            slug,
            issuer,
            client_id,
            client_secret_enc,
            scopes,
            email_claim,
            name_claim,
            subject_claim,
            auto_create_users: req.auto_create_users,
            auto_join_org_role,
            email_domain_allowlist,
            enabled: req.enabled,
            metadata: redirect_policy_metadata(&redirect_policy),
        })
        .await?;

    app.audit.emit(
        &ctx,
        "identity_provider.create",
        "identity_provider",
        Some(provider.id),
        Some(serde_json::json!({
            "slug": &provider.slug,
            "org_id": provider.org_id.map(|id| id.to_string()),
            "enabled": provider.enabled
        })),
    );

    Ok(Json(identity_provider_to_view(provider)))
}

pub(super) async fn update_identity_provider(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpdateIdentityProviderRequest>,
) -> AppResult<Json<IdentityProviderView>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let client_secret_enc = match req.client_secret {
        Some(secret) if !secret.trim().is_empty() => {
            Some(seal_idp_secret(&app, id.0, &secret).await?)
        }
        Some(_) => return Err(AppError::BadRequest("client_secret cannot be empty".into())),
        None => None,
    };
    let org_id = match req.org_id {
        Some(Some(raw)) => Some(parse_optional_org_id(Some(&raw))?),
        Some(None) => Some(None),
        None => None,
    };
    let auto_join_org_role = match req.auto_join_org_role {
        Some(role) => Some(normalize_optional_org_role(role)?),
        None => None,
    };
    let redirect_policy = req
        .redirect_policy
        .map(normalize_redirect_policy)
        .transpose()?;

    let provider = app
        .repos
        .identity_providers
        .update(
            id.0,
            IdentityProviderUpdate {
                org_id,
                name: req
                    .name
                    .map(|v| normalize_non_empty(v, "name"))
                    .transpose()?,
                slug: req.slug.map(|v| normalize_slug(&v)).transpose()?,
                issuer: req
                    .issuer
                    .map(|v| normalize_https_url(v, "issuer"))
                    .transpose()?,
                client_id: req
                    .client_id
                    .map(|v| normalize_non_empty(v, "client_id"))
                    .transpose()?,
                client_secret_enc,
                scopes: req.scopes.map(|v| normalize_scopes(Some(v))),
                email_claim: req
                    .email_claim
                    .map(|v| normalize_claim(Some(v), "email"))
                    .transpose()?,
                name_claim: req
                    .name_claim
                    .map(|v| normalize_claim(Some(v), "name"))
                    .transpose()?,
                subject_claim: req
                    .subject_claim
                    .map(|v| normalize_claim(Some(v), "sub"))
                    .transpose()?,
                auto_create_users: req.auto_create_users,
                auto_join_org_role,
                email_domain_allowlist: req
                    .email_domain_allowlist
                    .map(|v| normalize_domain_allowlist(Some(v)))
                    .transpose()?,
                enabled: req.enabled,
                metadata: redirect_policy.map(|p| redirect_policy_metadata(&p)),
            },
        )
        .await?;

    app.audit.emit(
        &ctx,
        "identity_provider.update",
        "identity_provider",
        Some(id.0),
        Some(serde_json::json!({
            "slug": &provider.slug,
            "enabled": provider.enabled
        })),
    );

    Ok(Json(identity_provider_to_view(provider)))
}

pub(super) async fn delete_identity_provider(
    State(app): State<AppState>,
    Path(id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    app.repos.identity_providers.soft_delete(id.0).await?;
    app.audit.emit(
        &ctx,
        "identity_provider.delete",
        "identity_provider",
        Some(id.0),
        None,
    );
    Ok(Json(serde_json::json!({"deleted": true})))
}

pub(super) async fn discover_identity_provider(
    State(app): State<AppState>,
    Authed(ctx): Authed,
    Json(req): Json<DiscoverIdentityProviderRequest>,
) -> AppResult<Json<DiscoverIdentityProviderResponse>> {
    require_user!(ctx);
    require!(ctx, Permission::PlatformAdmin, Scope::Platform);

    let issuer = normalize_https_url(req.issuer, "issuer")?;
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let resp = client
        .get(&discovery_url)
        .send()
        .await
        .map_err(|e| AppError::BadRequest(format!("OIDC discovery failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::BadRequest(format!(
            "OIDC discovery returned HTTP {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::BadRequest(format!("OIDC discovery JSON invalid: {e}")))?;

    let discovered_issuer = required_json_string(&body, "issuer")?;
    if discovered_issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return Err(AppError::BadRequest(
            "OIDC discovery issuer does not match requested issuer".into(),
        ));
    }
    let out = DiscoverIdentityProviderResponse {
        issuer: discovered_issuer,
        authorization_endpoint: required_json_string(&body, "authorization_endpoint")?,
        token_endpoint: required_json_string(&body, "token_endpoint")?,
        jwks_uri: required_json_string(&body, "jwks_uri")?,
        scopes_supported: body
            .get("scopes_supported")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .map(str::to_string)
            .collect(),
    };

    app.audit.emit(
        &ctx,
        "identity_provider.discover",
        "identity_provider",
        None,
        Some(serde_json::json!({"issuer": &out.issuer})),
    );

    Ok(Json(out))
}

impl Default for RedirectPolicyView {
    fn default() -> Self {
        Self {
            allow_relative: true,
            allowed_origins: vec![],
        }
    }
}

fn identity_provider_to_view(p: IdentityProviderRecord) -> IdentityProviderView {
    IdentityProviderView {
        id: p.id.to_string(),
        org_id: p.org_id.map(|id| id.to_string()),
        name: p.name,
        slug: p.slug,
        issuer: p.issuer,
        client_id: p.client_id,
        scopes: p.scopes,
        email_claim: p.email_claim,
        name_claim: p.name_claim,
        subject_claim: p.subject_claim,
        auto_create_users: p.auto_create_users,
        auto_join_org_role: p.auto_join_org_role,
        email_domain_allowlist: p.email_domain_allowlist,
        enabled: p.enabled,
        redirect_policy: redirect_policy_from_metadata(&p.metadata),
    }
}

async fn seal_idp_secret(app: &AppState, provider_id: Uuid, secret: &str) -> AppResult<Vec<u8>> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(AppError::BadRequest("client_secret is required".into()));
    }
    let crypto = app
        .crypto
        .as_ref()
        .ok_or_else(|| AppError::Internal("crypto KMS not configured".into()))?;
    let aad = gate_crypto::aad::idp_secret(provider_id);
    crypto
        .seal(secret.as_bytes(), &aad)
        .await
        .map_err(|e| AppError::Internal(format!("client_secret encrypt: {e}")))
}

fn normalize_non_empty(value: String, field: &str) -> AppResult<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(format!("{field} is required")));
    }
    Ok(trimmed)
}

fn normalize_slug(raw: &str) -> AppResult<String> {
    let slug = raw.trim().to_ascii_lowercase();
    if slug.is_empty()
        || slug.len() > 64
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || slug.starts_with('-')
        || slug.ends_with('-')
    {
        return Err(AppError::BadRequest(
            "slug must be 1-64 chars: lowercase letters, digits, hyphen".into(),
        ));
    }
    Ok(slug)
}

fn normalize_https_url(value: String, field: &str) -> AppResult<String> {
    let url = value.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("https://")
        || url.starts_with("http://localhost")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://[::1]"))
    {
        return Err(AppError::BadRequest(format!(
            "{field} must use https, except localhost development"
        )));
    }
    Ok(url)
}

fn normalize_scopes(scopes: Option<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = scopes
        .unwrap_or_default()
        .into_iter()
        .flat_map(|s| {
            s.split([',', ' ', '\n', '\t'])
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    if out.is_empty() {
        out = vec!["openid".into(), "email".into(), "profile".into()];
    }
    out.sort();
    out.dedup();
    out
}

fn normalize_claim(value: Option<String>, default: &str) -> AppResult<String> {
    let value = value.unwrap_or_else(|| default.to_string());
    normalize_non_empty(value, "claim")
}

fn normalize_optional_org_role(role: Option<String>) -> AppResult<Option<String>> {
    let Some(role) = role
        .map(|r| r.trim().to_ascii_lowercase())
        .filter(|r| !r.is_empty())
    else {
        return Ok(None);
    };
    let valid = ["owner", "admin", "billing_viewer", "member"];
    if !valid.contains(&role.as_str()) {
        return Err(AppError::BadRequest(format!(
            "auto_join_org_role must be one of: {valid:?}"
        )));
    }
    Ok(Some(role))
}

fn normalize_domain_allowlist(values: Option<Vec<String>>) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for value in values.unwrap_or_default() {
        for part in value.split([',', '\n', ' ', '\t']) {
            let domain = part.trim().trim_start_matches('@').to_ascii_lowercase();
            if domain.is_empty() {
                continue;
            }
            if domain.contains('/') || !domain.contains('.') {
                return Err(AppError::BadRequest(format!(
                    "invalid email domain: {domain}"
                )));
            }
            out.push(domain);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn normalize_redirect_policy(policy: RedirectPolicyView) -> AppResult<RedirectPolicyView> {
    let mut origins = Vec::new();
    for origin in policy.allowed_origins {
        let origin = normalize_origin(&origin)?;
        origins.push(origin);
    }
    origins.sort();
    origins.dedup();
    Ok(RedirectPolicyView {
        allow_relative: policy.allow_relative,
        allowed_origins: origins,
    })
}

fn normalize_origin(raw: &str) -> AppResult<String> {
    let value = raw.trim().trim_end_matches('/').to_ascii_lowercase();
    let (scheme, rest) = value
        .split_once("://")
        .ok_or_else(|| AppError::BadRequest("redirect origin must include scheme".into()))?;
    if scheme != "https" && scheme != "http" {
        return Err(AppError::BadRequest(
            "redirect origin scheme must be http or https".into(),
        ));
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err(AppError::BadRequest("redirect origin host invalid".into()));
    }
    Ok(format!("{scheme}://{host}"))
}

fn redirect_policy_metadata(policy: &RedirectPolicyView) -> serde_json::Value {
    serde_json::json!({
        "redirect_policy": {
            "allow_relative": policy.allow_relative,
            "allowed_origins": policy.allowed_origins,
        }
    })
}

fn redirect_policy_from_metadata(metadata: &serde_json::Value) -> RedirectPolicyView {
    let obj = metadata.get("redirect_policy").unwrap_or(metadata);
    RedirectPolicyView {
        allow_relative: obj
            .get("allow_relative")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        allowed_origins: obj
            .get("allowed_origins")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .filter_map(|v| normalize_origin(v).ok())
            .collect(),
    }
}

fn parse_optional_org_id(raw: Option<&str>) -> AppResult<Option<gate_core::id::OrgId>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    raw.parse::<gate_core::id::OrgId>()
        .map(Some)
        .map_err(|_| AppError::BadRequest("invalid org_id".into()))
}

fn required_json_string(body: &serde_json::Value, key: &str) -> AppResult<String> {
    body.get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest(format!("OIDC discovery missing {key}")))
}
