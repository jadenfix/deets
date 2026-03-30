#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

echo "==> Phase 7 Acceptance: CLI & tooling (Rust)"
cargo test -p aether-cli
cargo test -p aether-faucet
cargo test -p aether-scorecard

echo "==> Phase 7 Acceptance: Rust SDK tests"
cargo test -p aether-sdk --lib

echo "==> Phase 7 Acceptance: Python SDK tests"
pushd "${REPO_ROOT}/sdks/python" > /dev/null
if python3 -m pytest --version > /dev/null 2>&1; then
  PYTHONPATH=src python3 -m pytest
else
  echo "pytest not found, creating temporary virtualenv"
  python3 -m venv .venv
  source .venv/bin/activate
  pip install --upgrade pip >/dev/null
  pip install -e .[dev] >/dev/null
  PYTHONPATH=src python3 -m pytest
  deactivate
  rm -rf .venv
fi
popd > /dev/null

if [[ ! -d "${REPO_ROOT}/node_modules" ]]; then
  echo "==> Installing JavaScript workspace dependencies"
  pushd "${REPO_ROOT}" > /dev/null
  npm install --silent
  popd > /dev/null
fi

run_js_tests() {
  local project_dir="$1"
  local label="$2"
  echo "==> ${label}"
  pushd "${project_dir}" > /dev/null
  npm test
  popd > /dev/null
}

run_js_tests "${REPO_ROOT}/sdks/typescript" "Phase 7 Acceptance: TypeScript SDK tests"
run_js_tests "${REPO_ROOT}/packages/ui" "Phase 7 Acceptance: Shared UI tests"
run_js_tests "${REPO_ROOT}/apps/explorer" "Phase 7 Acceptance: Explorer app tests"
run_js_tests "${REPO_ROOT}/apps/wallet" "Phase 7 Acceptance: Wallet app tests"

echo "Phase 7 acceptance suite completed successfully."
