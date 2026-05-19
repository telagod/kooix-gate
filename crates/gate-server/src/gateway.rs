//! Gateway pipeline contracts.
//!
//! This is the shared skeleton for data-plane handlers (`chat`, `embeddings`,
//! `images`, `audio`).  The first pass keeps existing handlers intact but makes
//! stage names, failure policy, request correlation, and metering events typed so
//! settlement/replay logic can converge without hidden string contracts.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayStage {
    ResolveIdentity,
    Admission,
    Route,
    Adapt,
    Execute,
    Meter,
    Settle,
    AuditLog,
}

impl GatewayStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResolveIdentity => "resolve_identity",
            Self::Admission => "admission",
            Self::Route => "route",
            Self::Adapt => "adapt",
            Self::Execute => "execute",
            Self::Meter => "meter",
            Self::Settle => "settle",
            Self::AuditLog => "audit_log",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageOutcome {
    Ok,
    Error,
    Skipped,
}

impl StageOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    /// Reject the client request when the stage fails.
    FailClosed,
    /// Allow the request and record the failure for later reconciliation.
    FailOpen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayEvent {
    pub http_request_id: Uuid,
    pub provider_request_id: Option<String>,
    pub inflight_request_id: Option<Uuid>,
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub model_requested: String,
    pub model_actual: Option<String>,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeteringEvent {
    pub http_request_id: Uuid,
    pub provider_request_id: Option<String>,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub api_key_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub model_requested: String,
    pub model_actual: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub raw_cost_micros: i64,
    pub final_cost_micros: i64,
    pub status: u16,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayPipeline {
    pub billing_emit_policy: FailurePolicy,
    pub quota_policy: FailurePolicy,
}

impl Default for GatewayPipeline {
    fn default() -> Self {
        Self {
            // Current production semantics: billing is warn-only, quota Redis budget
            // failures are fail-open while hard auth/provider errors remain fail-closed.
            billing_emit_policy: FailurePolicy::FailOpen,
            quota_policy: FailurePolicy::FailOpen,
        }
    }
}

pub fn record_stage(stage: GatewayStage, outcome: StageOutcome, duration_secs: f64) {
    crate::metrics::record_gateway_stage(stage.as_str(), outcome.as_str(), duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_current_shadow_mode() {
        let p = GatewayPipeline::default();
        assert_eq!(p.billing_emit_policy, FailurePolicy::FailOpen);
        assert_eq!(p.quota_policy, FailurePolicy::FailOpen);
    }

    #[test]
    fn stage_labels_are_stable() {
        assert_eq!(GatewayStage::Route.as_str(), "route");
        assert_eq!(GatewayStage::Settle.as_str(), "settle");
        assert_eq!(StageOutcome::Error.as_str(), "error");
    }
}
