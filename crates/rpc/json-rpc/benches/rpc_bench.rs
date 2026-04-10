/// Criterion benchmarks for the JSON-RPC layer.
///
/// Groups:
/// 1. request_parsing  — serde_json deserialize JsonRpcRequest at varying payload sizes
/// 2. response_serial  — serialize JsonRpcResponse at varying result sizes
/// 3. rate_limiter     — async check() throughput: single IP, many IPs, at-limit
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use aether_rpc_json::{JsonRpcRequest, JsonRpcResponse, RateLimiter};

// ---------------------------------------------------------------------------
// 1. Request parsing
// ---------------------------------------------------------------------------

fn bench_request_parsing(c: &mut Criterion) {
    let mut g = c.benchmark_group("request_parsing");

    // Minimal request (no params)
    let minimal = r#"{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}"#;

    // Request with a hex-encoded transaction (~200 bytes)
    let tx_hex = "0x".to_string() + &"ab".repeat(100);
    let with_tx = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": "aeth_sendRawTransaction",
        "params": [tx_hex],
        "id": 42
    }))
    .unwrap();

    // Request with a large array param (~1 KB)
    let large_params: Vec<Value> = (0u64..64).map(|i| json!(format!("0x{:064x}", i))).collect();
    let large = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": "aeth_getBlockByNumber",
        "params": large_params,
        "id": 99
    }))
    .unwrap();

    for (label, payload) in [
        ("minimal", minimal.as_bytes().to_vec()),
        ("with_tx_100B", with_tx.into_bytes()),
        ("large_params_1KB", large.into_bytes()),
    ] {
        g.throughput(Throughput::Bytes(payload.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &payload, |b, p| {
            b.iter(|| {
                let _: JsonRpcRequest = serde_json::from_slice(p).expect("valid JSON-RPC request");
            });
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// 2. Response serialization
// ---------------------------------------------------------------------------

fn bench_response_serial(c: &mut Criterion) {
    let mut g = c.benchmark_group("response_serial");

    // Success — scalar result
    let scalar_ok = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        result: Some(json!(12345u64)),
        error: None,
        id: json!(1),
    };

    // Success — medium object (account state)
    let account_ok = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        result: Some(json!({
            "address": "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "balance": "1000000000000000000",
            "nonce": 7,
            "program": null
        })),
        error: None,
        id: json!(2),
    };

    // Error response
    let err_resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        result: None,
        error: Some(aether_rpc_json::JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
        id: json!(3),
    };

    for (label, resp) in [
        ("scalar_result", scalar_ok),
        ("account_result", account_ok),
        ("error_response", err_resp),
    ] {
        g.bench_with_input(BenchmarkId::from_parameter(label), &resp, |b, r| {
            b.iter(|| serde_json::to_string(r).unwrap());
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// 3. Rate-limiter throughput
// ---------------------------------------------------------------------------

fn bench_rate_limiter(c: &mut Criterion) {
    let mut g = c.benchmark_group("rate_limiter");
    // Use a very large token bucket so requests are never actually denied —
    // we're benchmarking the bookkeeping overhead, not the rejection path.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    // Single IP — tight loop on the same bucket entry.
    let limiter = RateLimiter::new(1_000_000, 1_000_000.0);
    let single_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    g.bench_function("single_ip_check", |b| {
        b.iter(|| {
            rt.block_on(limiter.check(single_ip));
        });
    });

    // 1 000 distinct IPs cycling — exercises HashMap insertions / lookups.
    let limiter2 = RateLimiter::new(1_000_000, 1_000_000.0);
    let ips: Vec<IpAddr> = (0u32..1_000)
        .map(|i| IpAddr::V4(Ipv4Addr::from(0x0A_00_00_00 | i)))
        .collect();
    let mut ip_idx: usize = 0;
    g.bench_function("1k_distinct_ips", |b| {
        b.iter(|| {
            let ip = ips[ip_idx % ips.len()];
            ip_idx = ip_idx.wrapping_add(1);
            rt.block_on(limiter2.check(ip));
        });
    });

    // cleanup() — iterate + retain over a 10 K entry map.
    let limiter3 = RateLimiter::new(1_000_000, 1_000_000.0);
    rt.block_on(async {
        for i in 0u32..10_000 {
            limiter3.check(IpAddr::V4(Ipv4Addr::from(i))).await;
        }
    });
    g.bench_function("cleanup_10k_entries", |b| {
        b.iter(|| {
            rt.block_on(limiter3.cleanup(Duration::from_secs(3600)));
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_request_parsing,
    bench_response_serial,
    bench_rate_limiter
);
criterion_main!(benches);
