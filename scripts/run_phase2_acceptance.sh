#!/usr/bin/env bash
set -euo pipefail

echo ":: Phase 2 acceptance – Economics & system programs"

echo ":: Staking lifecycle"
cargo test -p aether-program-staking -- --nocapture

echo ":: Governance proposals"
cargo test -p aether-program-governance -- --nocapture

echo ":: AMM invariant checks"
cargo test -p aether-program-amm -- --nocapture

echo ":: AIC token operations"
cargo test -p aether-program-aic-token -- --nocapture

echo ":: Job escrow flow"
cargo test -p aether-program-job-escrow -- --nocapture
