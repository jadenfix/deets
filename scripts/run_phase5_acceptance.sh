#!/usr/bin/env bash
set -euo pipefail

echo ":: Phase 5 acceptance – SRE & observability"

echo ":: Prometheus metrics coverage"
cargo test -p aether-metrics -- --nocapture

echo ":: QUIC transport instrumentation"
cargo test -p aether-quic-transport -- --nocapture
