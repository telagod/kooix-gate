//! ProjectRepo — Project 读写。

use crate::error::{DbError, DbResult};
use async_trait::async_trait;
use gate_core::id::{ChannelGroupId, OrgId, ProjectId};
use gate_core::identity::{Project, ProjectStatus};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[async_trait]
pub trait ProjectRepo: Send + Sync + 'static {
    async fn find_by_id(&self, id: ProjectId) -> DbResult<Project>;

    /// Org 维度列出所有 Project（只看 active）。
    async fn list_in_org(&self, org_id: OrgId) -> DbResult<Vec<Project>>;

    async fn create(&self, org_id: OrgId, name: &str, slug: &str) -> DbResult<Project>;

    async fn update(&self, id: ProjectId, name: Option<&str>, status: Option<&str>) -> DbResult<Project>;
}

pub struct PgProjectRepo {
    pool: PgPool,
}

impl PgProjectRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn parse_status(s: &str) -> DbResult<ProjectStatus> {
    match s {
        "active" => Ok(ProjectStatus::Active),
        "archived" => Ok(ProjectStatus::Archived),
        "deleted" => Ok(ProjectStatus::Deleted),
        other => Err(DbError::Internal(format!(
            "unknown project status: {other}"
        ))),
    }
}

fn row_to_project(row: &sqlx::postgres::PgRow) -> DbResult<Project> {
    let id: Uuid = row.try_get("id")?;
    let org: Uuid = row.try_get("org_id")?;
    let status: String = row.try_get("status")?;
    let default_group: Option<Uuid> = row.try_get("default_group_id")?;
    Ok(Project {
        id: ProjectId::from(id),
        org_id: OrgId::from(org),
        name: row.try_get("name")?,
        slug: row.try_get("slug")?,
        status: parse_status(&status)?,
        default_group_id: default_group.map(ChannelGroupId::from),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

const PROJECT_COLUMNS: &str = "id, org_id, name, slug, status, default_group_id, \
    created_at, updated_at";

#[async_trait]
impl ProjectRepo for PgProjectRepo {
    async fn find_by_id(&self, id: ProjectId) -> DbResult<Project> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects \
             WHERE id = $1 AND deleted_at IS NULL"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_project(&row)
    }

    async fn list_in_org(&self, org_id: OrgId) -> DbResult<Vec<Project>> {
        let rows = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects \
             WHERE org_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC"
        ))
        .bind(org_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_project).collect()
    }

    async fn create(&self, org_id: OrgId, name: &str, slug: &str) -> DbResult<Project> {
        let row = sqlx::query(&format!(
            "INSERT INTO projects (org_id, name, slug) \
             VALUES ($1, $2, $3) \
             RETURNING {PROJECT_COLUMNS}"
        ))
        .bind(org_id.as_uuid())
        .bind(name)
        .bind(slug)
        .fetch_one(&self.pool)
        .await?;
        row_to_project(&row)
    }

    async fn update(&self, id: ProjectId, name: Option<&str>, status: Option<&str>) -> DbResult<Project> {
        let row = sqlx::query(&format!(
            "UPDATE projects SET \
             name = COALESCE($2, name), \
             status = COALESCE($3, status), \
             updated_at = now() \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING {PROJECT_COLUMNS}"
        ))
        .bind(id.as_uuid())
        .bind(name)
        .bind(status)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)?;
        row_to_project(&row)
    }
}
