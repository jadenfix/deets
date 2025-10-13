#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

echo "==> Phase 7 Acceptance: Rust SDK tests"
cargo test -p aether-sdk --lib

echo "==> Phase 7 Acceptance: Python SDK tests"
pushd "${REPO_ROOT}/sdks/python" > /dev/null
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip >/dev/null
pip install -e .[dev] >/dev/null
pytest
deactivate
python3 - <<'PY'
import shutil
shutil.rmtree(".venv", ignore_errors=True)
PY
popd > /dev/null

echo "==> Phase 7 Acceptance: TypeScript SDK tests"
pushd "${REPO_ROOT}/sdks/typescript" > /dev/null
npm install --silent
npm test
popd > /dev/null

echo "Phase 7 acceptance suite completed successfully."
