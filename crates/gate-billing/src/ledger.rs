//! Immutable billing ledger event model.
//!
//! The ledger is the audit source of truth. `usage_records` remains an
//! analytics/read projection, while ledger rows carry explicit event semantics:
//! estimated debit, actual settle, refund, manual adjustment, and invoice close.

use crate::{BillingResult, UsageEvent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerEventType {
    EstimatedDebit,
    ActualSettle,
    Refund,
    ManualAdjustment,
    InvoiceClose,
}

impl LedgerEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EstimatedDebit => "estimated_debit",
            Self::ActualSettle => "actual_settle",
            Self::Refund => "refund",
            Self::ManualAdjustment => "manual_adjustment",
            Self::InvoiceClose => "invoice_close",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerDirection {
    Debit,
    Credit,
    None,
}

impl LedgerDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debit => "debit",
            Self::Credit => "credit",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerStatus {
    Pending,
    Posted,
    Voided,
}

impl LedgerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Posted => "posted",
            Self::Voided => "voided",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingLedgerEvent {
    pub id: Uuid,
    pub idempotency_key: String,
    pub request_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
    pub org_id: Uuid,
    pub project_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub event_type: LedgerEventType,
    pub direction: LedgerDirection,
    pub amount_micros: i64,
    pub source_type: String,
    pub source_id: String,
    pub status: LedgerStatus,
    pub invoice_month: Option<String>,
    pub metadata: serde_json::Value,
}

impl BillingLedgerEvent {
    pub fn actual_settle(event: &UsageEvent, idempotency_key: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            idempotency_key,
            request_id: Some(event.request_id),
            occurred_at: event.occurred_at,
            org_id: event.org_id,
            project_id: Some(event.project_id),
            api_key_id: Some(event.api_key_id),
            channel_id: event.channel_id,
            event_type: LedgerEventType::ActualSettle,
            direction: LedgerDirection::Debit,
            amount_micros: event.cost_micros,
            source_type: "llm_request".to_string(),
            source_id: event.request_id.to_string(),
            status: LedgerStatus::Posted,
            invoice_month: None,
            metadata: serde_json::json!({
                "model": event.model,
                "tokens_in": event.prompt_tokens,
                "tokens_out": event.completion_tokens,
                "tokens_cached": event.cached_tokens,
                "reasoning_tokens": event.reasoning_tokens,
                "image_units": event.image_units,
                "audio_seconds": event.audio_seconds,
                "raw_usage": event.raw_usage,
                "status": event.status,
                "group_id": event.group_id,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn estimated_debit(
        idempotency_key: String,
        request_id: Uuid,
        occurred_at: DateTime<Utc>,
        org_id: Uuid,
        project_id: Uuid,
        api_key_id: Uuid,
        channel_id: Option<Uuid>,
        amount_micros: i64,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            idempotency_key,
            request_id: Some(request_id),
            occurred_at,
            org_id,
            project_id: Some(project_id),
            api_key_id: Some(api_key_id),
            channel_id,
            event_type: LedgerEventType::EstimatedDebit,
            direction: LedgerDirection::Debit,
            amount_micros,
            source_type: "quota_predebit".to_string(),
            source_id: request_id.to_string(),
            status: LedgerStatus::Posted,
            invoice_month: None,
            metadata,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn refund(
        idempotency_key: String,
        request_id: Option<Uuid>,
        occurred_at: DateTime<Utc>,
        org_id: Uuid,
        project_id: Option<Uuid>,
        api_key_id: Option<Uuid>,
        amount_micros: i64,
        source_id: String,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            idempotency_key,
            request_id,
            occurred_at,
            org_id,
            project_id,
            api_key_id,
            channel_id: None,
            event_type: LedgerEventType::Refund,
            direction: LedgerDirection::Credit,
            amount_micros,
            source_type: "refund".to_string(),
            source_id,
            status: LedgerStatus::Posted,
            invoice_month: None,
            metadata,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn manual_adjustment(
        idempotency_key: String,
        occurred_at: DateTime<Utc>,
        org_id: Uuid,
        project_id: Option<Uuid>,
        amount_micros: i64,
        direction: LedgerDirection,
        source_id: String,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            idempotency_key,
            request_id: None,
            occurred_at,
            org_id,
            project_id,
            api_key_id: None,
            channel_id: None,
            event_type: LedgerEventType::ManualAdjustment,
            direction,
            amount_micros,
            source_type: "manual_adjustment".to_string(),
            source_id,
            status: LedgerStatus::Posted,
            invoice_month: None,
            metadata,
        }
    }

    pub fn invoice_close(
        idempotency_key: String,
        occurred_at: DateTime<Utc>,
        org_id: Uuid,
        invoice_month: String,
        amount_micros: i64,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            idempotency_key,
            request_id: None,
            occurred_at,
            org_id,
            project_id: None,
            api_key_id: None,
            channel_id: None,
            event_type: LedgerEventType::InvoiceClose,
            direction: LedgerDirection::None,
            amount_micros,
            source_type: "invoice".to_string(),
            source_id: invoice_month.clone(),
            status: LedgerStatus::Posted,
            invoice_month: Some(invoice_month),
            metadata,
        }
    }
}

pub async fn insert_ledger_event(pool: &PgPool, event: &BillingLedgerEvent) -> BillingResult<bool> {
    let mut tx = pool.begin().await?;
    let inserted = insert_ledger_event_tx(&mut tx, event).await?;
    tx.commit().await?;
    Ok(inserted)
}

pub(crate) async fn insert_ledger_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &BillingLedgerEvent,
) -> BillingResult<bool> {
    let inserted = sqlx::query_scalar::<_, bool>(
        "INSERT INTO billing_ledger_events \
         (id, idempotency_key, request_id, occurred_at, org_id, project_id, api_key_id, channel_id, \
          event_type, direction, amount_micros, source_type, source_id, status, invoice_month, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) \
         ON CONFLICT (idempotency_key) DO NOTHING \
         RETURNING TRUE",
    )
    .bind(event.id)
    .bind(&event.idempotency_key)
    .bind(event.request_id)
    .bind(event.occurred_at)
    .bind(event.org_id)
    .bind(event.project_id)
    .bind(event.api_key_id)
    .bind(event.channel_id)
    .bind(event.event_type.as_str())
    .bind(event.direction.as_str())
    .bind(event.amount_micros)
    .bind(&event.source_type)
    .bind(&event.source_id)
    .bind(event.status.as_str())
    .bind(&event.invoice_month)
    .bind(&event.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(false);
    Ok(inserted)
}
