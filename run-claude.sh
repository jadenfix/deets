#!/usr/bin/env bash
# ============================================================================
# run-claude.sh — Continuous autonomous Claude Code runner for Aether
# ============================================================================
# Usage:
#   ./run-claude.sh                    # Single agent on TASKS.md
#   ./run-claude.sh path/to/tasks.md   # Single agent on specific file
#   AGENTS=3 ./run-claude.sh           # 3 parallel agents on TASKS.md
#
# Environment:
#   MAX_HOURS=10      Max runtime in hours (default 10, ~overnight)
#   COOLDOWN=30       Seconds between cycles (default 30)
#   AGENTS=1          Number of parallel agents (default 1)
#   RATE_WAIT=300     Seconds to wait on rate limit (default 300 = 5 min)
#
# Kill switch:
#   touch /tmp/claude-runner-stop      # Gracefully stops after current cycle
# ============================================================================

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TASKS_FILE="${1:-${REPO_DIR}/TASKS.md}"
LOG_DIR="${REPO_DIR}/.claude/logs"
STOP_FILE="/tmp/claude-runner-stop"
MAX_HOURS="${MAX_HOURS:-10}"
COOLDOWN="${COOLDOWN:-30}"
AGENTS="${AGENTS:-1}"
RATE_WAIT="${RATE_WAIT:-300}"

AGENT_ID="${AGENT_ID:-1}"
LOCK_FILE="/tmp/claude-runner-agent${AGENT_ID}.lock"

# ── Guard against concurrent runs of same agent ID ──
if [ -f "$LOCK_FILE" ]; then
    EXISTING_PID=$(cat "$LOCK_FILE" 2>/dev/null || echo "")
    if [ -n "$EXISTING_PID" ] && kill -0 "$EXISTING_PID" 2>/dev/null; then
        echo "ERROR: Agent $AGENT_ID already running (PID ${EXISTING_PID})" >&2
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

# ── Launch parallel agents if AGENTS > 1 and we're the parent ──
if [ "$AGENTS" -gt 1 ] && [ "$AGENT_ID" -eq 1 ]; then
    echo "=== Launching $AGENTS parallel agents ===" | tee "${LOG_DIR}/runner.log"
    for i in $(seq 2 "$AGENTS"); do
        echo "Starting agent $i..." | tee -a "${LOG_DIR}/runner.log"
        AGENT_ID=$i AGENTS=1 "$0" "$TASKS_FILE" &
    done
    # Continue as agent 1
    AGENTS=1
fi

CYCLE=0
RUNNER_LOG="${LOG_DIR}/runner-agent${AGENT_ID}.log"

echo "=== Aether Agent $AGENT_ID ===" | tee "$RUNNER_LOG"
echo "Start:      $(date -Iseconds)" | tee -a "$RUNNER_LOG"
echo "Tasks:      $TASKS_FILE" | tee -a "$RUNNER_LOG"
echo "Max hours:  $MAX_HOURS" | tee -a "$RUNNER_LOG"
echo "Cooldown:   ${COOLDOWN}s between cycles" | tee -a "$RUNNER_LOG"
echo "Stop file:  $STOP_FILE" | tee -a "$RUNNER_LOG"
echo "==============================" | tee -a "$RUNNER_LOG"

cd "$REPO_DIR"

while true; do
    # ── Check stop conditions ──
    NOW=$(date +%s)
    if [ "$NOW" -ge "$DEADLINE" ]; then
        echo "[$(date -Iseconds)] Agent $AGENT_ID: Time limit ($MAX_HOURS hrs). Stopping." | tee -a "$RUNNER_LOG"
        break
    fi

    if [ -f "$STOP_FILE" ]; then
        echo "[$(date -Iseconds)] Agent $AGENT_ID: Stop file detected. Stopping." | tee -a "$RUNNER_LOG"
        break
    fi

    CYCLE=$((CYCLE + 1))
    TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
    LOG_FILE="${LOG_DIR}/agent${AGENT_ID}-cycle${CYCLE}-${TIMESTAMP}.log"

    echo "" | tee -a "$RUNNER_LOG"
    echo "[$(date -Iseconds)] Agent $AGENT_ID: === Cycle $CYCLE ===" | tee -a "$RUNNER_LOG"

    # ── Ensure we're on main with clean state ──
    git checkout main 2>&1 | tee -a "$RUNNER_LOG" || true
    git pull --ff-only 2>&1 | tee -a "$RUNNER_LOG" || true

    # ── Read task prompt, inject agent ID for coordination ──
    TASK_PROMPT="$(cat "$TASKS_FILE")

---
You are Agent $AGENT_ID of $AGENTS total agents. To avoid conflicts:
- Check PROGRESS.md and \`gh pr list --state all\` before picking a task.
- Include 'Agent $AGENT_ID' in your branch names: fix/agent${AGENT_ID}-<scope>-<description>
- If you see another agent already working on a task (open PR), skip it and pick the next one."

    echo "[$(date -Iseconds)] Agent $AGENT_ID: Starting claude..." | tee -a "$RUNNER_LOG"
    echo "Log: $LOG_FILE" | tee -a "$RUNNER_LOG"

    # ── Run claude ──
    caffeinate -dims \
        claude \
            --permission-mode bypassPermissions \
            --model claude-opus-4-6 \
            -p "$TASK_PROMPT" \
        >> "$LOG_FILE" 2>&1
    EXIT_CODE=$?

    echo "[$(date -Iseconds)] Agent $AGENT_ID: Cycle $CYCLE done (exit $EXIT_CODE)" | tee -a "$RUNNER_LOG"

    # ── Detect rate limiting ──
    if [ "$EXIT_CODE" -ne 0 ]; then
        if grep -qi 'rate.limit\|429\|overloaded\|capacity\|quota' "$LOG_FILE" 2>/dev/null; then
            echo "[$(date -Iseconds)] Agent $AGENT_ID: Rate limited. Waiting ${RATE_WAIT}s..." | tee -a "$RUNNER_LOG"
            sleep "$RATE_WAIT"
            continue
        fi
    fi

    # ── Notify ──
    if command -v osascript >/dev/null 2>&1; then
        osascript -e "display notification \"Agent $AGENT_ID cycle $CYCLE done (exit $EXIT_CODE)\" with title \"Aether Runner\"" 2>/dev/null || true
    fi

    # ── Cooldown ──
    echo "[$(date -Iseconds)] Agent $AGENT_ID: Cooldown ${COOLDOWN}s..." | tee -a "$RUNNER_LOG"
    sleep "$COOLDOWN"
done

echo "" | tee -a "$RUNNER_LOG"
echo "=== Agent $AGENT_ID Complete ===" | tee -a "$RUNNER_LOG"
echo "Finished: $(date -Iseconds)" | tee -a "$RUNNER_LOG"
echo "Cycles:   $CYCLE" | tee -a "$RUNNER_LOG"

if command -v osascript >/dev/null 2>&1; then
    osascript -e "display notification \"Agent $AGENT_ID done after $CYCLE cycles\" with title \"Aether Runner\"" 2>/dev/null || true
fi
