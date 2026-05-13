//! MembershipRepo — 一次性拉出用户所有角色快照（给 PgLoader 装 AuthContext 用）。
//!
//! 性能考量：登录/token 校验路径每次都要查 memberships，3000+ 活跃用户场景下
//! 用 3 条独立 SQL（org/project/platform）比 UNION 更直观，且 PG 会并行 plan。
//! 如果后续真成为热点，再考虑缓存到 Redis。

use crate::error::DbResult;
use async_trait::async_trait;
use gate_core::id::{OrgId, ProjectId, UserId};
use gate_core::identity::{OrgRole, PlatformRole, ProjectRole};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// 一个用户的全部角色快照 — PgLoader 会把这个直接塞进 AuthContext。
#[derive(Debug, Clone, Default)]
pub struct UserMemberships {
    pub orgs: HashMap<OrgId, OrgRole>,
    pub projects: HashMap<(OrgId, ProjectId), ProjectRole>,
    pub platform: Option<PlatformRole>,
}

#[async_trait]
pub trait MembershipRepo: Send + Sync + 'static {
    /// 一次性查出 user 的所有成员关系。
    async fn load_for_user(&self, user_id: UserId) -> DbResult<UserMemberships>;

    async fn add_org_member(&self, org: OrgId, user: UserId, role: OrgRole) -> DbResult<()>;

    async fn add_project_member(
        &self,
        project: ProjectId,
        user: UserId,
        role: ProjectRole,
    ) -> DbResult<()>;
}

pub struct PgMembershipRepo {
    pool: PgPool,
}

impl PgMembershipRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_org_role(s: &str) -> OrgRole {
    match s {
        "owner" => OrgRole::Owner,
        "admin" => OrgRole::Admin,
        "billing_viewer" => OrgRole::BillingViewer,
        _ => OrgRole::Member,
    }
}

fn parse_project_role(s: &str) -> ProjectRole {
    match s {
        "owner" => ProjectRole::Owner,
        "admin" => ProjectRole::Admin,
        "developer" => ProjectRole::Developer,
        _ => ProjectRole::Viewer,
    }
}

fn parse_platform_role(s: &str) -> Option<PlatformRole> {
    match s {
        "super_admin" => Some(PlatformRole::SuperAdmin),
        "operator" => Some(PlatformRole::Operator),
        "support" => Some(PlatformRole::Support),
        _ => None,
    }
}

fn org_role_to_str(r: OrgRole) -> &'static str {
    match r {
        OrgRole::Owner => "owner",
        OrgRole::Admin => "admin",
        OrgRole::BillingViewer => "billing_viewer",
        OrgRole::Member => "member",
    }
}

fn project_role_to_str(r: ProjectRole) -> &'static str {
    match r {
        ProjectRole::Owner => "owner",
        ProjectRole::Admin => "admin",
        ProjectRole::Developer => "developer",
        ProjectRole::Viewer => "viewer",
    }
}

#[async_trait]
impl MembershipRepo for PgMembershipRepo {
    async fn load_for_user(&self, user_id: UserId) -> DbResult<UserMemberships> {
        let uid = user_id.as_uuid();

        // Org memberships
        let org_rows = sqlx::query("SELECT org_id, role FROM org_memberships WHERE user_id = $1")
            .bind(uid)
            .fetch_all(&self.pool)
            .await?;
        let mut orgs = HashMap::with_capacity(org_rows.len());
        for r in &org_rows {
            let id: Uuid = r.try_get("org_id")?;
            let role: String = r.try_get("role")?;
            orgs.insert(OrgId::from(id), parse_org_role(&role));
        }

        // Project memberships — 需要带 org_id 以组合复合 key（防跨 Org 重放）
        let proj_rows = sqlx::query(
            "SELECT pm.project_id, pm.role, p.org_id \
             FROM project_memberships pm \
             JOIN projects p ON p.id = pm.project_id \
             WHERE pm.user_id = $1 AND p.deleted_at IS NULL",
        )
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        let mut projects = HashMap::with_capacity(proj_rows.len());
        for r in &proj_rows {
            let pid: Uuid = r.try_get("project_id")?;
            let oid: Uuid = r.try_get("org_id")?;
            let role: String = r.try_get("role")?;
            projects.insert(
                (OrgId::from(oid), ProjectId::from(pid)),
                parse_project_role(&role),
            );
        }

        // Platform role
        let platform_row = sqlx::query("SELECT role FROM platform_admins WHERE user_id = $1")
            .bind(uid)
            .fetch_optional(&self.pool)
            .await?;
        let platform = platform_row
            .and_then(|r| r.try_get::<String, _>("role").ok())
            .and_then(|s| parse_platform_role(&s));

        Ok(UserMemberships {
            orgs,
            projects,
            platform,
        })
    }

    async fn add_org_member(&self, org: OrgId, user: UserId, role: OrgRole) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO org_memberships (org_id, user_id, role) VALUES ($1, $2, $3) \
             ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(org.as_uuid())
        .bind(user.as_uuid())
        .bind(org_role_to_str(role))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn add_project_member(
        &self,
        project: ProjectId,
        user: UserId,
        role: ProjectRole,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO project_memberships (project_id, user_id, role) VALUES ($1, $2, $3) \
             ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(project.as_uuid())
        .bind(user.as_uuid())
        .bind(project_role_to_str(role))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
