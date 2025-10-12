#!/bin/bash
# Aether Blockchain - Test Script

set -e

echo "Running unit tests..."
cargo test --all-features --workspace

echo "Running doc tests..."
cargo test --doc --all-features --workspace

echo "Running integration tests..."
cargo test --test '*' --all-features

echo "All tests passed!"

