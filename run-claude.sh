#!/usr/bin/env bash
# ============================================================================
# run-claude.sh — Continuous autonomous Claude Code runner for Aether
# ============================================================================
# Usage:
#   ./run-claude.sh                    # Run TASKS.md in a loop until MAX_HOURS
#   ./run-claude.sh path/to/tasks.md   # Run specific task file
#
# Environment:
#   MAX_HOURS=10      Max runtime in hours (default 10, ~overnight)
#   MAX_TURNS=200     Max turns per claude session (default 200)
#   COOLDOWN=30       Seconds between cycles (default 30)
#
# Kill switch:
#   touch /tmp/claude-runner-stop      # Gracefully stops after current cycle
# ============================================================================

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TASKS_FILE="${1:-${REPO_DIR}/TASKS.md}"
LOG_DIR="${REPO_DIR}/.claude/logs"
LOCK_FILE="/tmp/claude-runner.lock"
STOP_FILE="/tmp/claude-runner-stop"
MAX_HOURS="${MAX_HOURS:-10}"
MAX_TURNS="${MAX_TURNS:-200}"
COOLDOWN="${COOLDOWN:-30}"

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

# ── Clear any stale stop file ──
rm -f "$STOP_FILE"

# ── Validate prerequisites ──
if [ ! -f "$TASKS_FILE" ]; then
    echo "ERROR: Tasks file not found: $TASKS_FILE" >&2
    exit 1
fi

for cmd in claude gh git cargo; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "ERROR: '$cmd' not found in PATH" >&2
        exit 1
    fi
done

mkdir -p "$LOG_DIR"

# ── Environment ──
export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

# ── Compute deadline ──
START_EPOCH=$(date +%s)
MAX_SECONDS=$(awk "BEGIN {printf \"%d\", $MAX_HOURS * 3600}")
DEADLINE=$((START_EPOCH + MAX_SECONDS))

CYCLE=0

echo "=== Aether Continuous Runner ===" | tee "${LOG_DIR}/runner.log"
echo "Start:      $(date -Iseconds)" | tee -a "${LOG_DIR}/runner.log"
echo "Tasks:      $TASKS_FILE" | tee -a "${LOG_DIR}/runner.log"
echo "Max hours:  $MAX_HOURS" | tee -a "${LOG_DIR}/runner.log"
echo "Max turns:  $MAX_TURNS per cycle" | tee -a "${LOG_DIR}/runner.log"
echo "Cooldown:   ${COOLDOWN}s between cycles" | tee -a "${LOG_DIR}/runner.log"
echo "Stop file:  $STOP_FILE" | tee -a "${LOG_DIR}/runner.log"
echo "================================" | tee -a "${LOG_DIR}/runner.log"

cd "$REPO_DIR"

while true; do
    # ── Check stop conditions ──
    NOW=$(date +%s)
    if [ "$NOW" -ge "$DEADLINE" ]; then
        echo "[$(date -Iseconds)] Time limit reached ($MAX_HOURS hours). Stopping." | tee -a "${LOG_DIR}/runner.log"
        break
    fi

    if [ -f "$STOP_FILE" ]; then
        echo "[$(date -Iseconds)] Stop file detected. Stopping." | tee -a "${LOG_DIR}/runner.log"
        rm -f "$STOP_FILE"
        break
    fi

    CYCLE=$((CYCLE + 1))
    TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
    LOG_FILE="${LOG_DIR}/run-${TIMESTAMP}-cycle${CYCLE}.log"

    echo "" | tee -a "${LOG_DIR}/runner.log"
    echo "[$(date -Iseconds)] === Cycle $CYCLE ===" | tee -a "${LOG_DIR}/runner.log"

    # ── Pull latest (pick up merged PRs) ──
    echo "[$(date -Iseconds)] Pulling latest..." | tee -a "${LOG_DIR}/runner.log"
    git pull --ff-only 2>&1 | tee -a "${LOG_DIR}/runner.log" || true

    # ── Read task prompt ──
    TASK_PROMPT="$(cat "$TASKS_FILE")"

    echo "[$(date -Iseconds)] Starting claude (max $MAX_TURNS turns)..." | tee -a "${LOG_DIR}/runner.log"
    echo "Log: $LOG_FILE" | tee -a "${LOG_DIR}/runner.log"

    # ── Run claude ──
    caffeinate -dims \
        claude \
            --permission-mode auto \
            --model claude-opus-4-6 \
            -p "$TASK_PROMPT" \
        >> "$LOG_FILE" 2>&1 || true

    EXIT_CODE=${PIPESTATUS[0]:-0}

    echo "[$(date -Iseconds)] Cycle $CYCLE finished (exit $EXIT_CODE)" | tee -a "${LOG_DIR}/runner.log"

    # ── Notify ──
    if command -v osascript >/dev/null 2>&1; then
        if [ "$EXIT_CODE" -eq 0 ]; then
            osascript -e "display notification \"Cycle $CYCLE complete\" with title \"Aether Runner\"" 2>/dev/null || true
        else
            osascript -e "display notification \"Cycle $CYCLE failed (exit $EXIT_CODE)\" with title \"Aether Runner\"" 2>/dev/null || true
        fi
    fi

    # ── Cooldown before next cycle ──
    echo "[$(date -Iseconds)] Cooling down ${COOLDOWN}s..." | tee -a "${LOG_DIR}/runner.log"
    sleep "$COOLDOWN"
done

echo "" | tee -a "${LOG_DIR}/runner.log"
echo "=== Runner Complete ===" | tee -a "${LOG_DIR}/runner.log"
echo "Finished: $(date -Iseconds)" | tee -a "${LOG_DIR}/runner.log"
echo "Cycles:   $CYCLE" | tee -a "${LOG_DIR}/runner.log"

if command -v osascript >/dev/null 2>&1; then
    osascript -e "display notification \"Runner done after $CYCLE cycles\" with title \"Aether Runner\"" 2>/dev/null || true
fi
