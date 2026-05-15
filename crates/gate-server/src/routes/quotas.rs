//! /v1/orgs/:org_id/quotas — Org-scoped quota CRUD（admin 视角）
//!
//! Endpoints:
//! - GET    /v1/orgs/:org_id/quotas              — 列出 Org 及其下属 project/api_key 的所有 quota
//! - POST   /v1/orgs/:org_id/quotas              — 创建/更新一行 quota（UPSERT）
//! - DELETE /v1/orgs/:org_id/quotas/:quota_id    — 硬删一行
//!
//! 权限：QuotaWrite / QuotaRead at `Scope::Org(&org)`。
//!
//! 越权防御（POST）：scope_id 必须可归属当前 Org：
//! - org：scope_id == org_id
//! - project：projects.org_id == org_id
//! - api_key：api_key 所属 project 的 org_id == org_id
//! - user / membership：直接拒绝（user 维度不在 Org 路径下管理）

use crate::auth::Authed;
use crate::error::{AppError, AppResult};
use crate::flex_uuid::FlexUuid;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::{
    Json, Router,
    routing::{delete, get},
};
use gate_auth::require;
use gate_core::id::{ApiKeyId, OrgId, ProjectId};
use gate_core::rbac::{Permission, Scope};
use gate_storage::{DbError, QuotaUpsert};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/orgs/:org_id/quotas", get(list_quotas).post(upsert_quota))
        .route("/orgs/:org_id/quotas/:quota_id", delete(delete_quota))
}

#[derive(Serialize)]
pub struct QuotaView {
    pub id: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub dimension: String,
    pub model_filter: Option<String>,
    pub limit_value: String,
    pub window_seconds: Option<i32>,
    pub enabled: bool,
}

impl From<gate_storage::QuotaRecord> for QuotaView {
    fn from(r: gate_storage::QuotaRecord) -> Self {
        Self {
            id: r.id.to_string(),
            scope_kind: r.scope_kind,
            scope_id: r.scope_id.to_string(),
            dimension: r.dimension,
            model_filter: r.model_filter,
            limit_value: r.limit_value.normalize().to_string(),
            window_seconds: r.window_seconds,
            enabled: r.enabled,
        }
    }
}

#[derive(Deserialize)]
pub struct UpsertQuotaRequest {
    pub scope_kind: String,
    pub scope_id: Uuid,
    pub dimension: String,
    pub model_filter: Option<String>,
    /// 数值字符串避免 JSON f64 精度损失（rust_decimal 用 serde-with-str）
    pub limit_value: Decimal,
    pub window_seconds: Option<i32>,
}

async fn list_quotas(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
) -> AppResult<Json<Vec<QuotaView>>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaRead, Scope::Org(&org));

    // Org 本身
    let mut out: Vec<QuotaView> = app
        .repos
        .quotas
        .list_by_scope("org", *org_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    // 下属 Project 的 quota
    let projects = app.repos.projects.list_in_org(org).await?;
    for p in &projects {
        let rows = app
            .repos
            .quotas
            .list_by_scope("project", *p.id.as_uuid())
            .await?;
        out.extend(rows.into_iter().map(Into::into));

        // Project 下的 api_keys
        let keys = app.repos.api_keys.list_in_project(p.id).await?;
        for k in &keys {
            let rows = app
                .repos
                .quotas
                .list_by_scope("api_key", *k.api_key_id.as_uuid())
                .await?;
            out.extend(rows.into_iter().map(Into::into));
        }
    }

    Ok(Json(out))
}

async fn upsert_quota(
    State(app): State<AppState>,
    Path(org_id): Path<FlexUuid>,
    Authed(ctx): Authed,
    Json(req): Json<UpsertQuotaRequest>,
) -> AppResult<Json<QuotaView>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaWrite, Scope::Org(&org));

    // 验 Org 存在
    let _ = app.repos.orgs.find_by_id(org).await?;

    // 维度白名单（schema CHECK 也会拦，提前拦能给 400 而非 db constraint）
    if !matches!(
        req.dimension.as_str(),
        "rpm"
            | "tpm"
            | "concurrent"
            | "daily_budget_usd"
            | "monthly_budget_usd"
            | "lifetime_tokens"
    ) {
        return Err(AppError::BadRequest(format!(
            "unknown dimension: {}",
            req.dimension
        )));
    }

    if req.limit_value < Decimal::ZERO {
        return Err(AppError::BadRequest("limit_value must be >= 0".into()));
    }

    // 越权写防御：scope_id 必须可归属当前 Org
    match req.scope_kind.as_str() {
        "org" => {
            if req.scope_id != *org_id {
                return Err(AppError::BadRequest(
                    "scope_id must equal path org_id when scope_kind=org".into(),
                ));
            }
        }
        "project" => {
            let p = app
                .repos
                .projects
                .find_by_id(ProjectId::from(req.scope_id))
                .await
                .map_err(|_| AppError::NotFound)?;
            if p.org_id != org {
                return Err(AppError::NotFound);
            }
        }
        "api_key" => {
            // 通过 api_keys list (project 维度) 反查不优雅，直接相信 record.org_id（来自 join 列）。
            // 但当前 ApiKeyRepo 只有 find_by_hash / list_in_project，没有 find_by_id。
            // 折中：列举 Org 下所有 project 的 keys，看 scope_id 是否在其中。
            let projects = app.repos.projects.list_in_org(org).await?;
            let target = ApiKeyId::from(req.scope_id);
            let mut found = false;
            for p in projects {
                let keys = app.repos.api_keys.list_in_project(p.id).await?;
                if keys.iter().any(|k| k.api_key_id == target) {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(AppError::NotFound);
            }
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "scope_kind '{other}' not manageable via Org endpoint (use platform admin)"
            )));
        }
    }

    let rec = app
        .repos
        .quotas
        .upsert(QuotaUpsert {
            scope_kind: req.scope_kind,
            scope_id: req.scope_id,
            dimension: req.dimension,
            model_filter: req.model_filter,
            limit_value: req.limit_value,
            window_seconds: req.window_seconds,
        })
        .await?;

    app.audit.emit(
        &ctx,
        "quota.upsert",
        "quota",
        Some(rec.id),
        Some(serde_json::json!({
            "scope_kind": &rec.scope_kind,
            "scope_id": rec.scope_id.to_string(),
            "dimension": &rec.dimension,
        })),
    );

    Ok(Json(rec.into()))
}

async fn delete_quota(
    State(app): State<AppState>,
    Path((org_id, quota_id)): Path<(FlexUuid, FlexUuid)>,
    Authed(ctx): Authed,
) -> AppResult<Json<serde_json::Value>> {
    let org = OrgId::from(org_id.0);
    require!(ctx, Permission::QuotaWrite, Scope::Org(&org));

    // 防越权：删除前先确认目标 quota 归属当前 Org
    // list 全部 Org 维度（含子层）然后定位 ID；规模可控因为 Org 级 quota 行数有限。
    let mut owned = false;
    for r in app.repos.quotas.list_by_scope("org", *org_id).await? {
        if r.id == *quota_id {
            owned = true;
            break;
        }
    }
    if !owned {
        for p in app.repos.projects.list_in_org(org).await? {
            for r in app
                .repos
                .quotas
                .list_by_scope("project", *p.id.as_uuid())
                .await?
            {
                if r.id == *quota_id {
                    owned = true;
                    break;
                }
            }
            if owned {
                break;
            }
            for k in app.repos.api_keys.list_in_project(p.id).await? {
                for r in app
                    .repos
                    .quotas
                    .list_by_scope("api_key", *k.api_key_id.as_uuid())
                    .await?
                {
                    if r.id == *quota_id {
                        owned = true;
                        break;
                    }
                }
                if owned {
                    break;
                }
            }
            if owned {
                break;
            }
        }
    }
    if !owned {
        return Err(AppError::NotFound);
    }

    match app.repos.quotas.delete(*quota_id).await {
        Ok(()) => {
            app.audit
                .emit(&ctx, "quota.delete", "quota", Some(*quota_id), None);
            Ok(Json(serde_json::json!({"deleted": true})))
        }
        Err(DbError::NotFound) => Err(AppError::NotFound),
        Err(e) => Err(e.into()),
    }
}
