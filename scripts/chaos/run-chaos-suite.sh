#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 1
  fi
}

require_cmd kubectl
require_cmd curl
require_cmd jq

echo "==> Chaos suite: packet loss"
"$SCRIPT_DIR/packet_loss_test.sh"

echo
echo "==> Chaos suite: network partition"
"$SCRIPT_DIR/partition_test.sh"

echo
echo "==> Chaos suite: crash recovery"
"$SCRIPT_DIR/crash_recovery_test.sh"
