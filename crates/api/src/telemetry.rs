//! Minimal, opt-in HTTP server tracing. Request content and headers are never span attributes.
use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use opentelemetry::{
    KeyValue, global,
    propagation::{Extractor, TextMapPropagator},
    trace::{Span, SpanKind, Tracer},
};
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};

/// Configure the exporter only when explicitly enabled. The standard OTEL exporter
/// environment variables configure endpoint, protocol and headers.
pub fn init() -> anyhow::Result<Option<SdkTracerProvider>> {
    if std::env::var("NULLBOX_OTEL_ENABLED").as_deref() != Ok("true") {
        return Ok(None);
    }
    let exporter = opentelemetry_otlp::SpanExporter::builder().with_http().build()?;
    let provider = SdkTracerProvider::builder().with_batch_exporter(exporter).build();
    global::set_tracer_provider(provider.clone());
    Ok(Some(provider))
}

// Only traceparent is consumed. In particular, never forward baggage or arbitrary headers.
struct TraceParent<'a>(&'a axum::http::HeaderMap);
impl Extractor for TraceParent<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        if key.eq_ignore_ascii_case("traceparent") { self.0.get("traceparent")?.to_str().ok() } else { None }
    }
    fn keys(&self) -> Vec<&str> {
        vec!["traceparent"]
    }
}

pub async fn trace_request(request: Request, next: Next) -> Response {
    let tracer = global::tracer("nullbox.http");
    let parent = TraceContextPropagator::new().extract(&TraceParent(request.headers()));
    let route = request.extensions().get::<MatchedPath>().map(MatchedPath::as_str).unwrap_or("<unmatched>").to_owned();
    let method = request.method().as_str().to_owned();
    let builder = tracer
        .span_builder(format!("{method} {route}"))
        .with_kind(SpanKind::Server)
        .with_attributes(vec![KeyValue::new("http.request.method", method), KeyValue::new("http.route", route)]);
    let mut span = tracer.build_with_context(builder, &parent);
    let response = next.run(request).await;
    span.set_attribute(KeyValue::new("http.response.status_code", i64::from(response.status().as_u16())));
    span.end();
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::post,
    };
    use opentelemetry_sdk::trace::InMemorySpanExporter;
    use tower::ServiceExt;

    #[tokio::test]
    async fn server_span_inherits_trace_and_excludes_request_secrets() {
        let _subscriber = tracing::subscriber::set_default(
            tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_writer(std::io::sink).finish(),
        );
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder().with_simple_exporter(exporter.clone()).build();
        global::set_tracer_provider(provider.clone());
        let app = Router::new()
            .route(
                "/api/tickets",
                post(|| async {
                    tracing::info!("private-log-marker");
                    StatusCode::CREATED
                }),
            )
            .layer(axum::middleware::from_fn(trace_request))
            .layer(tower_http::trace::TraceLayer::new_for_http());
        let request = Request::builder()
            .method("POST")
            .uri("/api/tickets?secret=query-secret")
            .header("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .header("authorization", "Bearer header-secret")
            .body(Body::from("body-secret"))
            .unwrap();
        assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::CREATED);
        let spans = exporter.get_finished_spans().unwrap();
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(span.span_context.trace_id().to_string(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(span.parent_span_id.to_string(), "00f067aa0ba902b7");
        assert_eq!(span.span_kind, SpanKind::Server);
        let rendered = format!("{span:?}");
        assert!(rendered.contains("/api/tickets"));
        assert!(rendered.contains("201"));
        for secret in ["query-secret", "header-secret", "body-secret", "private-log-marker"] {
            assert!(!rendered.contains(secret));
        }
        provider.shutdown().unwrap();
    }
}
