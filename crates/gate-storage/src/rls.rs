//! Row-Level Security context helpers.
//!
//! PostgreSQL RLS policies (defined in `20260513000010_rls.sql`) rely on session
//! variables (`app.current_org_id`, `app.current_project_id`, `app.is_platform_admin`)
//! to filter rows. This module provides:
//!
//! - [`RlsContext`] — a lightweight struct holding the values to inject.
//! - [`RlsContext::begin`] — starts a transaction with RLS variables pre-applied.
//!
//! # Usage
//!
//! ```ignore
//! let rls = RlsContext::for_org(org_id);
//! let mut tx = rls.begin(&pool).await?;
//! let rows = sqlx::query("SELECT * FROM projects")
//!     .fetch_all(&mut *tx)
//!     .await?;
//! tx.commit().await?;
//! ```
//!
//! # Activation path
//!
//! Phase 1 (this commit): RlsContext is stored in request extensions by the RLS
//! middleware. Individual repo methods that need defense-in-depth can opt-in to
//! `begin()`. The existing repo layer continues to work unmodified (the superuser
//! connection bypasses RLS by default in PostgreSQL when no role switch is done).
//!
//! Phase 2 (future): switch the pool to connect as `gate_app`, making RLS mandatory
//! for all queries. At that point `begin()` becomes the standard path and the
//! middleware ensures every connection has variables set.

use gate_core::id::{OrgId, ProjectId};
use sqlx::PgPool;

/// Session-level RLS parameters extracted from the authenticated request context.
#[derive(Debug, Clone)]
pub struct RlsContext {
    pub org_id: Option<OrgId>,
    pub project_id: Option<ProjectId>,
    pub is_platform_admin: bool,
}

impl RlsContext {
    /// Convenience: context for a platform admin (bypasses all RLS policies).
    pub fn platform_admin() -> Self {
        Self {
            org_id: None,
            project_id: None,
            is_platform_admin: true,
        }
    }

    /// Convenience: context scoped to a single org.
    pub fn for_org(org_id: OrgId) -> Self {
        Self {
            org_id: Some(org_id),
            project_id: None,
            is_platform_admin: false,
        }
    }

    /// Convenience: context scoped to org + project.
    pub fn for_project(org_id: OrgId, project_id: ProjectId) -> Self {
        Self {
            org_id: Some(org_id),
            project_id: Some(project_id),
            is_platform_admin: false,
        }
    }

    /// Begin a transaction with RLS session variables pre-applied via `SET LOCAL`.
    ///
    /// `SET LOCAL` scopes the variables to this transaction — once committed or
    /// rolled back, the connection returns to a clean state for pool reuse.
    ///
    /// Callers must `.commit().await?` on success; `Drop` rolls back automatically.
    pub async fn begin<'a>(
        &self,
        pool: &'a PgPool,
    ) -> Result<sqlx::Transaction<'a, sqlx::Postgres>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        if let Some(org_id) = &self.org_id {
            let stmt = format!("SET LOCAL app.current_org_id = '{}'", org_id.as_uuid());
            sqlx::query(&stmt).execute(&mut *tx).await?;
        }
        if let Some(project_id) = &self.project_id {
            let stmt = format!(
                "SET LOCAL app.current_project_id = '{}'",
                project_id.as_uuid()
            );
            sqlx::query(&stmt).execute(&mut *tx).await?;
        }
        if self.is_platform_admin {
            sqlx::query("SET LOCAL app.is_platform_admin = 'true'")
                .execute(&mut *tx)
                .await?;
        }

        Ok(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::id::OrgId;

    #[test]
    fn platform_admin_context() {
        let ctx = RlsContext::platform_admin();
        assert!(ctx.is_platform_admin);
        assert!(ctx.org_id.is_none());
    }

    #[test]
    fn org_context() {
        let org = OrgId::new();
        let ctx = RlsContext::for_org(org);
        assert_eq!(ctx.org_id.unwrap(), org);
        assert!(!ctx.is_platform_admin);
    }
}
