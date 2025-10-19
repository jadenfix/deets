#!/usr/bin/env bash
set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
cd "$WORKSPACE_ROOT"

echo "[phase2] Running fmt check"
cargo fmt --all -- --check

echo "[phase2] Building workspace"
cargo build --workspace

echo "[phase2] Running core integration suites"
cargo test -p aether-runtime --tests
cargo test -p aether-ledger --tests
cargo test -p aether-state-snapshots --tests
cargo test -p aether-state-storage --tests

# cross-crate integration validation
cargo test --test phase2_integration

# determinism validation
cargo test --test determinism_test

# targeted CLI smoke tests relevant to runtime changes
cargo test -p aether-cli transfer_and_job_flow -- --exact

echo "Phase 2 acceptance tests completed successfully"
