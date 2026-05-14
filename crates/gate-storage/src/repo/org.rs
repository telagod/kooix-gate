//! OrgRepo — Organization 读写。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use gate_core::id::{OrgId, UserId};
use gate_core::identity::{OrgStatus, Organization};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[async_trait]
pub trait OrgRepo: Send + Sync + 'static {
    async fn find_by_id(&self, id: OrgId) -> DbResult<Organization>;
    async fn find_by_slug(&self, slug: &str) -> DbResult<Organization>;

    /// 返回 user 可见的所有 Org（作为 owner 或 org_memberships 成员）。
    async fn list_for_user(&self, user_id: UserId) -> DbResult<Vec<Organization>>;

    /// 平台管理员：列出所有 Org。
    async fn list_all(&self) -> DbResult<Vec<Organization>>;

    async fn create(&self, name: &str, slug: &str, owner: UserId) -> DbResult<Organization>;

    async fn update(&self, id: OrgId, name: Option<&str>, billing_email: Option<&str>) -> DbResult<Organization>;
}

pub struct PgOrgRepo {
    pool: PgPool,
}

impl PgOrgRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_status(s: &str) -> DbResult<OrgStatus> {
    match s {
        "active" => Ok(OrgStatus::Active),
        "suspended" => Ok(OrgStatus::Suspended),
        "deleted" => Ok(OrgStatus::Deleted),
        other => Err(DbError::Internal(format!("unknown org status: {other}"))),
    }
}

fn row_to_org(row: &sqlx::postgres::PgRow) -> DbResult<Organization> {
    let id: Uuid = row.try_get("id")?;
    let owner: Uuid = row.try_get("owner_user_id")?;
    let status: String = row.try_get("status")?;
    Ok(Organization {
        id: OrgId::from(id),
        name: row.try_get("name")?,
        slug: row.try_get("slug")?,
        owner_user_id: UserId::from(owner),
        status: parse_status(&status)?,
        billing_email: row.try_get("billing_email")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

const ORG_COLUMNS: &str = "id, name, slug, owner_user_id, status, billing_email, \
    created_at, updated_at";

#[async_trait]
impl OrgRepo for PgOrgRepo {
    async fn find_by_id(&self, id: OrgId) -> DbResult<Organization> {
        let row = sqlx::query(&format!(
            "SELECT {ORG_COLUMNS} FROM organizations \
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_org(&row)
    }

    async fn find_by_slug(&self, slug: &str) -> DbResult<Organization> {
        let row = sqlx::query(&format!(
            "SELECT {ORG_COLUMNS} FROM organizations \
             WHERE slug = $1 AND deleted_at IS NULL"
        ))
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_org(&row)
    }

    async fn list_for_user(&self, user_id: UserId) -> DbResult<Vec<Organization>> {
        let rows = sqlx::query(&format!(
            "SELECT DISTINCT {} FROM organizations o \
             LEFT JOIN org_memberships m ON m.org_id = o.id \
             WHERE o.deleted_at IS NULL \
             AND (o.owner_user_id = $1 OR m.user_id = $1) \
             ORDER BY o.created_at DESC",
            ORG_COLUMNS
                .split(", ")
                .map(|c| format!("o.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_org).collect()
    }

    async fn create(&self, name: &str, slug: &str, owner: UserId) -> DbResult<Organization> {
        let row = sqlx::query(&format!(
            "INSERT INTO organizations (name, slug, owner_user_id) \
             VALUES ($1, $2, $3) \
             RETURNING {ORG_COLUMNS}"
        ))
        .bind(name)
        .bind(slug)
        .bind(owner.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        row_to_org(&row)
    }

    async fn list_all(&self) -> DbResult<Vec<Organization>> {
        let rows = sqlx::query(&format!(
            "SELECT {ORG_COLUMNS} FROM organizations WHERE deleted_at IS NULL ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_org).collect()
    }

    async fn update(&self, id: OrgId, name: Option<&str>, billing_email: Option<&str>) -> DbResult<Organization> {
        let row = sqlx::query(&format!(
            "UPDATE organizations SET \
             name = COALESCE($2, name), \
             billing_email = COALESCE($3, billing_email), \
             updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING {ORG_COLUMNS}"
        ))
        .bind(id.as_uuid())
        .bind(name)
        .bind(billing_email)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_org(&row)
    }
}
