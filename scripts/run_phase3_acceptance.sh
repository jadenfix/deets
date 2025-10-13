#!/usr/bin/env bash
set -euo pipefail

echo ":: Phase 3 acceptance – AI mesh & verifiable compute"

echo ":: Runtime containers & scheduling"
cargo test -p aether-ai-runtime -- --nocapture

echo ":: TEE attestation verification"
cargo test -p aether-verifiers-tee -- --nocapture

echo ":: VCR validator consistency"
cargo test -p aether-verifiers-vcr -- --nocapture

echo ":: KZG verifier plumbing"
cargo test -p aether-verifiers-kzg -- --nocapture

echo ":: Reputation scoring"
cargo test -p aether-program-reputation -- --nocapture
