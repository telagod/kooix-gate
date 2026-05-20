//! 基础 middleware: request_id / trace / catch_panic

use axum::extract::Request;
use axum::http::{Request as HttpRequest, Response as HttpResponse};
use axum::middleware::Next;
use axum::response::Response;
use std::time::Duration;
use tower_http::classify::{ServerErrorsAsFailures, ServerErrorsFailureClass, SharedClassifier};
use tower_http::trace::{
    DefaultOnBodyChunk, DefaultOnEos, MakeSpan, OnFailure, OnRequest, OnResponse, TraceLayer,
};
use tracing::{Level, Span};
use uuid::Uuid;

use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KooixRequestId(pub Uuid);

pub async fn request_id_extension(mut req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_request_id)
        .unwrap_or_else(Uuid::now_v7);
    req.extensions_mut().insert(KooixRequestId(request_id));
    next.run(req).await
}

fn parse_request_id(raw: &str) -> Option<Uuid> {
    Uuid::parse_str(raw)
        .ok()
        .or_else(|| Uuid::parse_str(raw.trim_start_matches("req_")).ok())
}

pub fn request_id_layers() -> (SetRequestIdLayer<MakeRequestUuid>, PropagateRequestIdLayer) {
    let header = axum::http::HeaderName::from_static("x-request-id");
    (
        SetRequestIdLayer::new(header.clone(), MakeRequestUuid),
        PropagateRequestIdLayer::new(header),
    )
}

type KooixTraceLayer = TraceLayer<
    SharedClassifier<ServerErrorsAsFailures>,
    KooixMakeSpan,
    KooixOnRequest,
    KooixOnResponse,
    DefaultOnBodyChunk,
    DefaultOnEos,
    KooixOnFailure,
>;

pub fn trace_layer() -> KooixTraceLayer {
    TraceLayer::new_for_http()
        .make_span_with(KooixMakeSpan)
        .on_request(KooixOnRequest)
        .on_response(KooixOnResponse)
        .on_failure(KooixOnFailure)
}

#[derive(Debug, Clone, Copy)]
pub struct KooixMakeSpan;

impl<B> MakeSpan<B> for KooixMakeSpan {
    fn make_span(&mut self, request: &HttpRequest<B>) -> Span {
        tracing::span!(
            Level::INFO,
            "http.request",
            method = %request.method(),
            uri = %request.uri(),
            version = ?request.version(),
            request_id = tracing::field::Empty,
            status = tracing::field::Empty,
            latency_ms = tracing::field::Empty,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KooixOnRequest;

impl<B> OnRequest<B> for KooixOnRequest {
    fn on_request(&mut self, request: &HttpRequest<B>, span: &Span) {
        crate::trace_context::attach_http_request_attrs(span, request.headers());
        tracing::debug!("started processing request");
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KooixOnResponse;

impl<B> OnResponse<B> for KooixOnResponse {
    fn on_response(self, response: &HttpResponse<B>, latency: Duration, span: &Span) {
        crate::trace_context::record_http_response(span, response.status().as_u16(), latency);
        tracing::debug!(
            status = response.status().as_u16(),
            latency_ms = latency.as_millis(),
            "finished processing request"
        );
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KooixOnFailure;

impl OnFailure<ServerErrorsFailureClass> for KooixOnFailure {
    fn on_failure(&mut self, failure: ServerErrorsFailureClass, latency: Duration, span: &Span) {
        span.record("latency_ms", latency.as_millis() as u64);
        tracing::error!(
            classification = %failure,
            latency_ms = latency.as_millis(),
            "response failed"
        );
    }
}
