#!/usr/bin/env bash
set -euo pipefail

echo ":: Phase 1 acceptance – Core ledger, consensus, mempool"

echo ":: Ledger state transitions"
cargo test -p aether-ledger -- --nocapture

echo ":: Consensus finality & leader election"
cargo test -p aether-consensus -- --nocapture

echo ":: Mempool admission & QoS"
cargo test -p aether-mempool -- --nocapture
