#!/usr/bin/env bash
set -euo pipefail

echo ":: Phase 6 acceptance – Security & audits readiness"

echo ":: KES key evolution & rotation"
cargo test -p aether-crypto-kes -- --nocapture

echo ":: Ed25519 primitives"
cargo test -p aether-crypto-primitives -- --nocapture

echo ":: BLS aggregation & verification"
cargo test -p aether-crypto-bls -- --nocapture

echo ":: KZG polynomial commitments"
cargo test -p aether-crypto-kzg -- --nocapture

echo ":: VRF randomness & eligibility"
cargo test -p aether-crypto-vrf -- --nocapture
