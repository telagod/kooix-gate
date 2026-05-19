//! 基础 middleware: request_id / trace / catch_panic

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

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

pub fn trace_layer()
-> TraceLayer<tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>>
{
    TraceLayer::new_for_http()
}
