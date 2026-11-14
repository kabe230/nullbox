use opentelemetry::{
    Context,
    trace::{Tracer, TracerProvider},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[tokio::test]
async fn otlp_http_uses_standard_base_endpoint_and_protobuf() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        let mut data = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
            if let Some(end) = data.windows(4).position(|x| x == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                let length: usize =
                    headers.lines().find_map(|line| line.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                if data.len() >= end + 4 + length {
                    break;
                }
            }
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        data
    });
    // This integration-test binary has a single test and no other environment users.
    unsafe {
        std::env::set_var("NULLBOX_OTEL_ENABLED", "true");
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{addr}/api/otlp"));
        std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf");
        std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", "Authorization=Bearer%20test-token");
    }
    let provider = nullbox_api::telemetry::init().unwrap().unwrap();
    let tracer = provider.tracer("test");
    let span = tracer.start_with_context("test-span", &Context::new());
    drop(span);
    provider.force_flush().unwrap();
    let data = server.join().unwrap();
    let end = data.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
    assert!(headers.starts_with("post /api/otlp/v1/traces http/1.1"));
    assert!(headers.contains("content-type: application/x-protobuf"));
    assert!(headers.contains("authorization: bearer test-token"));
    assert!(data[end + 4..].windows(9).any(|w| w == b"test-span"));
    provider.shutdown().unwrap();
}
