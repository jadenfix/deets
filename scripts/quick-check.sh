#!/usr/bin/env bash
set -euo pipefail

echo ":: Running quick Phase 1 validation suite"

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

echo "=> fmt check"
cargo fmt -- --check

echo "=> consensus unit tests (hybrid, hotstuff, simple)"
cargo test -p aether-consensus hybrid::
cargo test -p aether-consensus hotstuff::
cargo test -p aether-consensus simple::

echo "=> runtime + node phase 1 tests"
cargo test -p aether-node --test phase1_acceptance

echo "=> multi-validator integration"
cargo test -p aether-node --test multi_validator_test

echo "✅ quick-check complete"
