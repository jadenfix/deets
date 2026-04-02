use once_cell::sync::Lazy;
use prometheus::{
    register_histogram_vec, register_int_counter, register_int_counter_vec, HistogramVec,
    IntCounter, IntCounterVec,
};

pub struct RpcMetrics {
    /// Total RPC requests received (by method).
    pub requests_total: IntCounterVec,
    /// Total RPC errors returned (by method).
    pub errors_total: IntCounterVec,
    /// RPC request latency in seconds (by method).
    pub request_duration_seconds: HistogramVec,
    /// Total requests rejected by rate limiter.
    pub rate_limited_total: IntCounter,
}

impl RpcMetrics {
    fn new() -> Self {
        RpcMetrics {
            requests_total: register_int_counter_vec!(
                "aether_rpc_requests_total",
                "Total JSON-RPC requests by method",
                &["method"]
            )
            .expect("register rpc requests_total"),
            errors_total: register_int_counter_vec!(
                "aether_rpc_errors_total",
                "Total JSON-RPC error responses by method",
                &["method"]
            )
            .expect("register rpc errors_total"),
            request_duration_seconds: register_histogram_vec!(
                "aether_rpc_request_duration_seconds",
                "JSON-RPC request latency in seconds",
                &["method"],
                vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
            )
            .expect("register rpc request_duration_seconds"),
            rate_limited_total: register_int_counter!(
                "aether_rpc_rate_limited_total",
                "Total requests rejected by rate limiter"
            )
            .expect("register rpc rate_limited_total"),
        }
    }
}

pub static RPC_METRICS: Lazy<RpcMetrics> = Lazy::new(RpcMetrics::new);
