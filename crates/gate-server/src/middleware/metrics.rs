//! HTTP metrics middleware — records request count, duration, and status code.
//!
//! Sits in the middleware stack and measures every request that passes through.
//! Metrics are emitted via the `metrics` facade and collected by the Prometheus
//! recorder installed at startup.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::time::Instant;

/// Metrics recording middleware. Records request count and duration.
///
/// Mount early in the stack (outside rate_limit) to capture ALL requests
/// including those that get 429'd.
pub async fn metrics_layer(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    metrics::gauge!("gate_active_requests").increment(1.0);
    let response = next.run(req).await;
    metrics::gauge!("gate_active_requests").decrement(1.0);

    let status = response.status().as_u16();
    let duration = start.elapsed().as_secs_f64();

    crate::metrics::record_request(&method, &path, status, duration);

    response
}
