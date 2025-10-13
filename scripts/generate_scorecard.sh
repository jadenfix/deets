#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <metrics.json> [markdown_out] [csv_out]" >&2
  exit 1
fi

INPUT=$1
MARKDOWN=${2:-out/scorecard.md}
CSV=${3:-out/scorecard.csv}

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

mkdir -p "$(dirname "$MARKDOWN")"
mkdir -p "$(dirname "$CSV")"

cargo run --manifest-path "${REPO_ROOT}/Cargo.toml" -p aether-scorecard --bin scorecard -- \
  --input "$INPUT" \
  --markdown-out "$MARKDOWN" \
  --csv-out "$CSV"
