-- ============================================================================
-- Identity: Users / Organizations / Projects / Memberships
-- ============================================================================

-- 用户（全局账户）
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email           CITEXT NOT NULL UNIQUE,
    password_hash   TEXT,                       -- NULL = SSO 用户
    display_name    TEXT,
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'suspended', 'deleted', 'pending_verification')),
    mfa_secret      TEXT,                       -- TOTP secret, encrypted at rest
    mfa_enabled     BOOLEAN NOT NULL DEFAULT FALSE,
    email_verified_at TIMESTAMPTZ,
    last_login_at   TIMESTAMPTZ,
    last_login_ip   INET,
    failed_logins   INT NOT NULL DEFAULT 0,
    locked_until    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX users_status_idx ON users(status) WHERE deleted_at IS NULL;
CREATE TRIGGER users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 组织（顶层租户，SaaS 模式下计费/合同主体）
CREATE TABLE organizations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    slug            CITEXT NOT NULL UNIQUE,
    owner_user_id   UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'suspended', 'deleted')),
    billing_email   CITEXT,
    metadata        JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX organizations_owner_idx ON organizations(owner_user_id);
CREATE TRIGGER organizations_updated_at BEFORE UPDATE ON organizations
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- 项目（隔离主体）
CREATE TABLE projects (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id              UUID NOT NULL REFERENCES organizations(id) ON DELETE RESTRICT,
    name                TEXT NOT NULL,
    slug                CITEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active'
                        CHECK (status IN ('active', 'archived', 'deleted')),
    default_group_id    UUID,                   -- 延迟外键，channel_groups 之后建
    metadata            JSONB NOT NULL DEFAULT '{}'::JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at          TIMESTAMPTZ,
    UNIQUE (org_id, slug)
);

CREATE INDEX projects_org_idx ON projects(org_id) WHERE deleted_at IS NULL;
CREATE TRIGGER projects_updated_at BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Org 级成员（owner / admin / billing_viewer / member）
CREATE TABLE org_memberships (
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL
                    CHECK (role IN ('owner', 'admin', 'billing_viewer', 'member')),
    invited_by      UUID REFERENCES users(id),
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, user_id)
);

CREATE INDEX org_memberships_user_idx ON org_memberships(user_id);

-- Project 级成员（owner / admin / developer / viewer）
CREATE TABLE project_memberships (
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL
                    CHECK (role IN ('owner', 'admin', 'developer', 'viewer')),
    invited_by      UUID REFERENCES users(id),
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX project_memberships_user_idx ON project_memberships(user_id);

-- Platform 级角色（运营 / SuperAdmin）
CREATE TABLE platform_admins (
    user_id         UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL
                    CHECK (role IN ('super_admin', 'operator', 'support')),
    granted_by      UUID REFERENCES users(id),
    granted_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 邀请（待接受）
CREATE TABLE invitations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_kind      TEXT NOT NULL CHECK (scope_kind IN ('org', 'project')),
    scope_id        UUID NOT NULL,              -- org_id 或 project_id
    email           CITEXT NOT NULL,
    role            TEXT NOT NULL,
    token_hash      TEXT NOT NULL UNIQUE,       -- 邀请 token 的 SHA-256
    invited_by      UUID NOT NULL REFERENCES users(id),
    expires_at      TIMESTAMPTZ NOT NULL,
    accepted_at     TIMESTAMPTZ,
    accepted_by     UUID REFERENCES users(id),
    revoked_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX invitations_scope_idx ON invitations(scope_kind, scope_id) WHERE accepted_at IS NULL AND revoked_at IS NULL;
CREATE INDEX invitations_email_idx ON invitations(email) WHERE accepted_at IS NULL AND revoked_at IS NULL;
