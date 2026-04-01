#!/usr/bin/env bash
# ============================================================================
# run-claude.sh — Unattended Claude Code runner for Aether blockchain
# ============================================================================
# Usage:
#   ./run-claude.sh                    # Run TASKS.md
#   ./run-claude.sh path/to/tasks.md   # Run specific task file
# ============================================================================

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TASKS_FILE="${1:-${REPO_DIR}/TASKS.md}"
LOG_DIR="${REPO_DIR}/.claude/logs"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
LOG_FILE="${LOG_DIR}/run-${TIMESTAMP}.log"
LOCK_FILE="/tmp/claude-runner.lock"

# ── Guard against concurrent runs ──
if [ -f "$LOCK_FILE" ]; then
    EXISTING_PID=$(cat "$LOCK_FILE" 2>/dev/null || echo "")
    if [ -n "$EXISTING_PID" ] && kill -0 "$EXISTING_PID" 2>/dev/null; then
        echo "ERROR: Another claude-runner is already running (PID ${EXISTING_PID})" >&2
        exit 1
    fi
    rm -f "$LOCK_FILE"
fi
echo $$ > "$LOCK_FILE"
trap 'rm -f "$LOCK_FILE"' EXIT

# ── Validate prerequisites ──
if [ ! -f "$TASKS_FILE" ]; then
    echo "ERROR: Tasks file not found: $TASKS_FILE" >&2
    exit 1
fi

if ! command -v claude >/dev/null 2>&1; then
    echo "ERROR: 'claude' CLI not found in PATH" >&2
    exit 1
fi

mkdir -p "$LOG_DIR"

# ── Environment ──
export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

TASK_PROMPT="$(cat "$TASKS_FILE")"

echo "=== Claude Code Runner ===" | tee "$LOG_FILE"
echo "Start:    $(date -Iseconds)" | tee -a "$LOG_FILE"
echo "Tasks:    $TASKS_FILE" | tee -a "$LOG_FILE"
echo "Log:      $LOG_FILE" | tee -a "$LOG_FILE"
echo "Repo:     $REPO_DIR" | tee -a "$LOG_FILE"
echo "Model:    claude-opus-4-6 (1M context)" | tee -a "$LOG_FILE"
echo "=========================" | tee -a "$LOG_FILE"

cd "$REPO_DIR"
caffeinate -dims \
    claude \
        --permission-mode auto \
        --model claude-opus-4-6 \
        -p "$TASK_PROMPT" \
    2>&1 | tee -a "$LOG_FILE"

EXIT_CODE=${PIPESTATUS[0]}

echo "" | tee -a "$LOG_FILE"
echo "=== Run Complete ===" | tee -a "$LOG_FILE"
echo "Finished: $(date -Iseconds)" | tee -a "$LOG_FILE"
echo "Exit:     ${EXIT_CODE}" | tee -a "$LOG_FILE"

if command -v osascript >/dev/null 2>&1; then
    if [ "$EXIT_CODE" -eq 0 ]; then
        osascript -e 'display notification "Claude Code run completed successfully" with title "Aether CI"'
    else
        osascript -e 'display notification "Claude Code run FAILED (exit '"$EXIT_CODE"')" with title "Aether CI"'
    fi
fi

exit "$EXIT_CODE"
