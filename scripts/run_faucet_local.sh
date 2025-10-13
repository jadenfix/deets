#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

export RUST_LOG=${RUST_LOG:-info}
export AETHER_FAUCET_ADDR=${AETHER_FAUCET_ADDR:-127.0.0.1:8080}

cargo run --manifest-path "${REPO_ROOT}/Cargo.toml" -p aether-faucet --bin server
