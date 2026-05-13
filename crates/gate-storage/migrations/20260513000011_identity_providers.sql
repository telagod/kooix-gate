-- ============================================================================
-- SSO / OIDC: 身份提供者 + 用户身份绑定
-- ============================================================================

-- 身份提供者：可以是平台级（org_id NULL，如 Google/GitHub）或 Org 级（企业自有 Okta/Azure AD）
CREATE TABLE identity_providers (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id              UUID REFERENCES organizations(id) ON DELETE CASCADE,
    provider_type       TEXT NOT NULL DEFAULT 'oidc'
                        CHECK (provider_type IN ('oidc', 'saml')),   -- saml 留扩展位
    name                TEXT NOT NULL,                                -- 'Google', 'Okta-prod' 等
    slug                TEXT NOT NULL,                                -- URL 友好标识，用于 /login/sso/{slug}

    -- OIDC 配置
    issuer              TEXT NOT NULL,                                -- 用于 OIDC discovery
    client_id           TEXT NOT NULL,
    client_secret_enc   BYTEA NOT NULL,                               -- envelope encrypted
    scopes              TEXT[] NOT NULL DEFAULT ARRAY['openid', 'email', 'profile'],

    -- Claim 映射
    email_claim         TEXT NOT NULL DEFAULT 'email',
    name_claim          TEXT NOT NULL DEFAULT 'name',
    subject_claim       TEXT NOT NULL DEFAULT 'sub',

    -- JIT provisioning
    auto_create_users   BOOLEAN NOT NULL DEFAULT TRUE,
    auto_join_org_role  TEXT CHECK (auto_join_org_role IN ('owner', 'admin', 'billing_viewer', 'member')),
    email_domain_allowlist TEXT[] NOT NULL DEFAULT '{}', -- 限制邮箱域

    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    metadata            JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at          TIMESTAMPTZ,

    -- 平台级 slug 全局唯一；Org 级 slug 在 Org 内唯一
    UNIQUE (org_id, slug)
);

CREATE UNIQUE INDEX identity_providers_platform_slug_idx
    ON identity_providers(slug) WHERE org_id IS NULL AND deleted_at IS NULL;
CREATE INDEX identity_providers_org_idx
    ON identity_providers(org_id) WHERE org_id IS NOT NULL AND deleted_at IS NULL;

CREATE TRIGGER identity_providers_updated_at BEFORE UPDATE ON identity_providers
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 用户绑定的外部身份（一个用户可绑多个 provider，也可以同时有密码）
CREATE TABLE user_identities (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider_id     UUID NOT NULL REFERENCES identity_providers(id) ON DELETE CASCADE,
    subject         TEXT NOT NULL,                          -- provider 返回的 sub
    email_at_link   CITEXT,
    raw_claims      JSONB NOT NULL DEFAULT '{}'::JSONB,     -- 最近一次的完整 claims，调试用
    linked_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at   TIMESTAMPTZ,
    UNIQUE (provider_id, subject)
);

CREATE INDEX user_identities_user_idx ON user_identities(user_id);

-- OIDC 登录中态：保存 PKCE verifier + nonce + redirect target
-- 用 token (URL 安全 hash) 在前端 cookie 里，避免 stateless cookie 过大
CREATE TABLE oidc_login_states (
    state_hash      TEXT PRIMARY KEY,                       -- SHA-256(state token)
    provider_id     UUID NOT NULL REFERENCES identity_providers(id) ON DELETE CASCADE,
    pkce_verifier   TEXT NOT NULL,
    nonce           TEXT NOT NULL,
    redirect_to     TEXT,                                   -- 登录后跳转目标
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ NOT NULL
);

CREATE INDEX oidc_login_states_expires_idx ON oidc_login_states(expires_at);

-- 让 RLS 兜底 user_identities（Org 隔离）
ALTER TABLE identity_providers ENABLE ROW LEVEL SECURITY;
CREATE POLICY identity_providers_isolation ON identity_providers
    USING (is_platform_admin() OR org_id IS NULL OR org_id = current_org_id());
