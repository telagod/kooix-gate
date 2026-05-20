//! Low-cardinality trace helpers for HTTP, data-plane, upstream and billing spans.

use axum::http::HeaderMap;
use gate_auth::{AuthContext, Subject};
use std::time::Duration;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceIdentity {
    pub request_id: Uuid,
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

impl TraceIdentity {
    pub fn from_auth(ctx: &AuthContext, request_id: Uuid) -> Self {
        let (org_id, project_id, api_key_id, user_id) = match ctx.subject() {
            Some(Subject::ApiKey {
                api_key_id,
                project_id,
                org_id,
            }) => (
                Some(*org_id.as_uuid()),
                Some(*project_id.as_uuid()),
                Some(*api_key_id.as_uuid()),
                None,
            ),
            Some(Subject::User { user_id, .. }) => (
                ctx.current_org().map(|id| *id.as_uuid()),
                None,
                None,
                Some(*user_id.as_uuid()),
            ),
            _ => (None, None, None, None),
        };
        Self {
            request_id,
            org_id,
            project_id,
            api_key_id,
            user_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlaneTrace {
    pub identity: TraceIdentity,
    pub endpoint: &'static str,
    pub provider_type: String,
    pub channel_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub model: String,
}

impl DataPlaneTrace {
    pub fn new(
        identity: TraceIdentity,
        endpoint: &'static str,
        provider_type: impl Into<String>,
        channel_id: Option<Uuid>,
        group_id: Option<Uuid>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            identity,
            endpoint,
            provider_type: provider_type.into(),
            channel_id,
            group_id,
            model: model.into(),
        }
    }

    pub fn span(&self) -> Span {
        let span = tracing::info_span!(
            "gateway.data_plane",
            request_id = %self.identity.request_id,
            org_id = display_opt_uuid(self.identity.org_id),
            project_id = display_opt_uuid(self.identity.project_id),
            api_key_id = display_opt_uuid(self.identity.api_key_id),
            user_id = display_opt_uuid(self.identity.user_id),
            endpoint = self.endpoint,
            provider_type = %self.provider_type,
            channel_id = display_opt_uuid(self.channel_id),
            group_id = display_opt_uuid(self.group_id),
            model = %self.model,
        );
        attach_identity_attrs(&span, &self.identity);
        span.set_attribute("kooix.endpoint", self.endpoint);
        span.set_attribute("kooix.provider_type", self.provider_type.clone());
        set_opt_uuid_attr(&span, "kooix.channel_id", self.channel_id);
        set_opt_uuid_attr(&span, "kooix.group_id", self.group_id);
        span.set_attribute("kooix.model", self.model.clone());
        span
    }

    pub fn upstream_span(&self, operation: &'static str, streaming: bool) -> Span {
        let span = tracing::info_span!(
            "gateway.upstream_request",
            request_id = %self.identity.request_id,
            org_id = display_opt_uuid(self.identity.org_id),
            project_id = display_opt_uuid(self.identity.project_id),
            api_key_id = display_opt_uuid(self.identity.api_key_id),
            endpoint = self.endpoint,
            operation = operation,
            provider_type = %self.provider_type,
            channel_id = display_opt_uuid(self.channel_id),
            group_id = display_opt_uuid(self.group_id),
            model = %self.model,
            streaming = streaming,
            outcome = tracing::field::Empty,
            duration_ms = tracing::field::Empty,
        );
        attach_identity_attrs(&span, &self.identity);
        span.set_attribute("kooix.endpoint", self.endpoint);
        span.set_attribute("kooix.operation", operation);
        span.set_attribute("kooix.provider_type", self.provider_type.clone());
        set_opt_uuid_attr(&span, "kooix.channel_id", self.channel_id);
        set_opt_uuid_attr(&span, "kooix.group_id", self.group_id);
        span.set_attribute("kooix.model", self.model.clone());
        span.set_attribute("kooix.streaming", streaming);
        span
    }
}

pub fn record_upstream_outcome(span: &Span, status: &'static str, elapsed: Duration) {
    span.record("outcome", status);
    span.record("duration_ms", elapsed.as_millis() as u64);
    span.set_attribute("kooix.outcome", status);
    span.set_attribute("kooix.duration_ms", elapsed.as_millis() as i64);
}

pub fn attach_http_request_attrs(span: &Span, headers: &HeaderMap) {
    if let Some(request_id) = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_request_id)
    {
        span.record("request_id", request_id.to_string());
        span.set_attribute("kooix.request_id", request_id.to_string());
    }
}

pub fn attach_identity_attrs(span: &Span, identity: &TraceIdentity) {
    span.set_attribute("kooix.request_id", identity.request_id.to_string());
    set_opt_uuid_attr(span, "kooix.org_id", identity.org_id);
    set_opt_uuid_attr(span, "kooix.project_id", identity.project_id);
    set_opt_uuid_attr(span, "kooix.api_key_id", identity.api_key_id);
    set_opt_uuid_attr(span, "kooix.user_id", identity.user_id);
}

pub fn record_http_response(span: &Span, status: u16, latency: Duration) {
    span.record("status", status);
    span.record("latency_ms", latency.as_millis() as u64);
    span.set_attribute("http.response.status_code", status as i64);
    span.set_attribute("kooix.latency_ms", latency.as_millis() as i64);
}

fn parse_request_id(raw: &str) -> Option<Uuid> {
    Uuid::parse_str(raw)
        .ok()
        .or_else(|| Uuid::parse_str(raw.trim_start_matches("req_")).ok())
}

fn display_opt_uuid(value: Option<Uuid>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn set_opt_uuid_attr(span: &Span, key: &'static str, value: Option<Uuid>) {
    if let Some(value) = value {
        span.set_attribute(key, value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_auth::AuthContext;
    use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
    use std::collections::HashMap;

    #[test]
    fn trace_identity_extracts_api_key_scope() {
        let api_key_id = ApiKeyId::new();
        let project_id = ProjectId::new();
        let org_id = OrgId::new();
        let request_id = Uuid::now_v7();
        let identity = TraceIdentity::from_auth(
            &AuthContext::api_key(api_key_id, project_id, org_id),
            request_id,
        );

        assert_eq!(identity.request_id, request_id);
        assert_eq!(identity.org_id, Some(*org_id.as_uuid()));
        assert_eq!(identity.project_id, Some(*project_id.as_uuid()));
        assert_eq!(identity.api_key_id, Some(*api_key_id.as_uuid()));
        assert_eq!(identity.user_id, None);
    }

    #[test]
    fn trace_identity_extracts_user_scope() {
        let user_id = UserId::new();
        let org_id = OrgId::new();
        let request_id = Uuid::now_v7();
        let identity = TraceIdentity::from_auth(
            &AuthContext::user(
                user_id,
                Uuid::now_v7(),
                HashMap::new(),
                HashMap::new(),
                None,
                Some(org_id),
            ),
            request_id,
        );

        assert_eq!(identity.request_id, request_id);
        assert_eq!(identity.org_id, Some(*org_id.as_uuid()));
        assert_eq!(identity.project_id, None);
        assert_eq!(identity.api_key_id, None);
        assert_eq!(identity.user_id, Some(*user_id.as_uuid()));
    }
}
