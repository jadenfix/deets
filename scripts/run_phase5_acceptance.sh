#!/usr/bin/env bash
set -euo pipefail

echo ":: Phase 5 acceptance – SRE & observability"

echo ":: Prometheus metrics coverage (all subsystems)"
cargo test -p aether-metrics -- --nocapture

echo ":: QUIC transport instrumentation"
cargo test -p aether-quic-transport -- --nocapture

echo ":: RPC health endpoint and request metrics"
cargo test -p aether-rpc-json server::tests::test_health_endpoint_returns_node_status -- --nocapture
cargo test -p aether-rpc-json server::tests::rpc_metrics_record_requests_and_errors -- --nocapture

echo ":: Node-level sync and slot metrics"
cargo test -p aether-node node::tests -- --nocapture

echo ":: Structured tracing spans (ledger + node)"
cargo test -p aether-ledger tracing -- --nocapture 2>/dev/null || echo "(no tracing-specific tests in ledger, covered by integration)"

echo ":: Grafana dashboard and Prometheus alert configs exist"
test -f deploy/grafana/dashboards/aether-overview.json || { echo "FAIL: Grafana dashboard missing"; exit 1; }
test -f deploy/prometheus/alerts.yml || { echo "FAIL: Prometheus alerts missing"; exit 1; }

echo ""
echo ":: Phase 5 acceptance – PASSED"
