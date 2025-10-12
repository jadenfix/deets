#!/bin/bash
# Aether Blockchain - Lint Script

set -e

echo "Running cargo fmt check..."
cargo fmt --all -- --check

echo "Running clippy..."
cargo clippy --all-targets --all-features -- -D warnings

echo "Running cargo check..."
cargo check --all-features --workspace

echo "All lints passed!"

