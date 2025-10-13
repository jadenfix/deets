// Prometheus Metrics HTTP Exporter
use anyhow::{Context, Result};
use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use prometheus::{Encoder, TextEncoder};
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::{info, warn};

/// Start Prometheus metrics HTTP exporter
///
/// Serves metrics on /metrics endpoint (standard Prometheus format)
///
/// Usage:
/// ```no_run
/// use aether_metrics::exporter::start_metrics_exporter;
///
/// #[tokio::main]
/// async fn main() {
///     let addr = "127.0.0.1:9090".parse().unwrap();
///     start_metrics_exporter(addr).await.unwrap();
/// }
/// ```
pub async fn start_metrics_exporter(addr: SocketAddr) -> Result<()> {
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(metrics_handler))
    });

    let server = Server::bind(&addr).serve(make_svc);

    info!("Prometheus metrics exporter listening on http://{}/metrics", addr);

    server
        .await
        .context("Metrics exporter server failed")?;

    Ok(())
}

/// HTTP handler for /metrics endpoint
async fn metrics_handler(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    // Collect all registered metrics
    let metric_families = prometheus::gather();
    
    // Encode to Prometheus text format
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    
    match encoder.encode(&metric_families, &mut buffer) {
        Ok(_) => {
            let response = Response::builder()
                .status(200)
                .header(CONTENT_TYPE, encoder.format_type())
                .body(Body::from(buffer))
                .unwrap();
            
            Ok(response)
        }
        Err(e) => {
            warn!("Failed to encode metrics: {}", e);
            
            let response = Response::builder()
                .status(500)
                .body(Body::from(format!("Error encoding metrics: {}", e)))
                .unwrap();
            
            Ok(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_handler() {
        // Increment some metrics
        crate::CONSENSUS_METRICS.slots_finalized.inc();
        crate::RUNTIME_METRICS.tx_executed.inc();
        
        // Create test request
        let req = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        
        // Call handler
        let response = metrics_handler(req).await.unwrap();
        
        // Verify response
        assert_eq!(response.status(), 200);
        
        // Read body
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body = String::from_utf8(body_bytes.to_vec()).unwrap();
        
        // Verify metrics are present
        assert!(body.contains("aether_consensus_slots_finalized"));
        assert!(body.contains("aether_runtime_tx_executed"));
    }
}

