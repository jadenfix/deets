#!/usr/bin/env bash
set -euo pipefail

echo ":: Running Phase 4 acceptance suite"

echo ":: ed25519 batch verification throughput"
cargo test -p aether-crypto-primitives ed25519::tests::test_phase4_batch_performance -- --ignored --nocapture

echo ":: BLS aggregated verification throughput"
cargo test -p aether-crypto-bls verify::tests::test_phase4_bls_batch_performance -- --ignored --nocapture

echo ":: Turbine packet-loss resilience"
cargo test -p aether-da-turbine tests::phase4_acceptance_turbine_packet_loss_resilience -- --nocapture

echo ":: Snapshot catch-up benchmark"
cargo test -p aether-state-snapshots importer::tests::phase4_snapshot_catch_up_benchmark -- --ignored --nocapture
