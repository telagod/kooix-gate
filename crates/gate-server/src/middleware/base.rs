//! 基础 middleware: request_id / trace / catch_panic

use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

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
