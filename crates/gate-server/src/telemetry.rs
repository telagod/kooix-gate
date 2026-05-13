//! OpenTelemetry tracing initialization.
//!
//! If `OTEL_EXPORTER_OTLP_ENDPOINT` is set, configures an OTLP exporter that
//! sends trace spans to the configured collector (Jaeger, Tempo, etc.).
//! If the env var is absent, telemetry is silently skipped (no-op).
//!
//! The returned [`TracerProvider`] must be kept alive for the duration of the
//! process and shut down gracefully on exit.

use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::TracerProvider;

/// Initialize OpenTelemetry tracing with OTLP export.
///
/// Returns `Some(TracerProvider)` if `OTEL_EXPORTER_OTLP_ENDPOINT` is set,
/// `None` otherwise (telemetry disabled).
///
/// Caller must hold the returned provider and call `provider.shutdown()` on
/// graceful exit.
pub fn init_telemetry(service_name: &str) -> Option<TracerProvider> {
    // Only activate when the standard env var is present.
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok()?;

    tracing::info!("initializing OpenTelemetry OTLP exporter");

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
    {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "failed to build OTLP exporter; telemetry disabled");
            return None;
        }
    };

    let resource = opentelemetry_sdk::Resource::new(vec![
        KeyValue::new("service.name", service_name.to_string()),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION").to_string()),
    ]);

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource)
        .build();

    Some(provider)
}

/// Build a `tracing_opentelemetry::OpenTelemetryLayer` from a provider.
///
/// The layer is generic over the subscriber `S`, so it can be composed with
/// any subscriber stack (e.g. `Registry + EnvFilter + fmt`).
pub fn otel_layer<S>(
    provider: &TracerProvider,
) -> tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use opentelemetry::trace::TracerProvider as _;
    let tracer = provider.tracer("gate-server");
    tracing_opentelemetry::layer().with_tracer(tracer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_env_returns_none() {
        // Ensure OTEL_EXPORTER_OTLP_ENDPOINT is not set (it shouldn't be in CI).
        // If it happens to be set, skip this test.
        if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok() {
            return;
        }
        assert!(init_telemetry("test-svc").is_none());
    }
}
