#!/usr/bin/env bash
# ============================================================================
# run-claude.sh — Autonomous engineering team for Aether blockchain
# ============================================================================
# Launches N specialized agents, each in its own git worktree, with distinct
# roles. Agents coordinate via PROGRESS.md and gh pr list. No race conditions.
#
# Usage:
#   AGENTS=5 ./run-claude.sh           # 5-agent engineering team
#   ./run-claude.sh                    # Single agent (all roles)
#
# Environment:
#   MAX_HOURS=10      Max runtime (default 10)
#   COOLDOWN=30       Seconds between cycles (default 30)
#   AGENTS=1          Number of agents (default 1)
#   RATE_WAIT=300     Rate limit backoff (default 300s)
#
# Kill switch:   touch /tmp/claude-runner-stop
# ============================================================================

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="${REPO_DIR}/.claude/logs"
STOP_FILE="/tmp/claude-runner-stop"
MAX_HOURS="${MAX_HOURS:-10}"
COOLDOWN="${COOLDOWN:-30}"
AGENTS="${AGENTS:-1}"
RATE_WAIT="${RATE_WAIT:-300}"

mkdir -p "$LOG_DIR"
rm -f "$STOP_FILE"

# ── Environment ──
export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

# ── Compute deadline ──
START_EPOCH=$(date +%s)
MAX_SECONDS=$(awk "BEGIN {printf \"%d\", $MAX_HOURS * 3600}")
DEADLINE=$((START_EPOCH + MAX_SECONDS))

# ── Agent role definitions ──
# Each agent gets a specialized prompt focused on their area of expertise.
# They coordinate through PROGRESS.md (shared memory) and gh pr list.

agent_prompt() {
    local AGENT_ID=$1
    local TOTAL=$2
    local WORK_DIR=$3
    local TASKS
    TASKS=$(cat "${REPO_DIR}/TASKS.md")

    local ROLE=""
    local FOCUS=""

    case $AGENT_ID in
        1)
            ROLE="Lead Engineer — Correctness & Safety"
            FOCUS="You own Tier 1 (transaction safety, double-spend, block validation, overflow, nonce, WASM gas).
These are consensus-breaking bugs. Read the ledger, node, and runtime code carefully. Write thorough tests.
If you finish Tier 1, move to Tier 4 (storage & persistence)."
            ;;
        2)
            ROLE="Consensus Engineer"
            FOCUS="You own Tier 2 (HotStuff liveness, slashing enforcement, fork choice, epoch transitions, finality).
Deep-dive into crates/consensus/ and crates/node/src/fork_choice.rs. Ensure BFT guarantees hold.
If you finish Tier 2, move to Tier 5 (testing — write multi-node and Byzantine fault tests)."
            ;;
        3)
            ROLE="Networking & P2P Engineer"
            FOCUS="You own Tier 3 (state sync, peer banning, message limits, graceful shutdown, backpressure).
Focus on crates/p2p/, crates/networking/, and crates/node/src/sync.rs.
If you finish Tier 3, help with Tier 6 (docker compose genesis ceremony)."
            ;;
        4)
            ROLE="Test & Quality Engineer"
            FOCUS="You own Tier 5 (integration tests, proptests, Byzantine tests, benchmarks).
Write tests that prove the system works. Add proptest for transactions and merkle proofs.
Write multi-node integration tests. Add criterion benchmarks.
If you finish Tier 5, help with any remaining Tier 1-2 items."
            ;;
        5)
            ROLE="Platform & Ops Engineer"
            FOCUS="You own Tier 6 (Prometheus metrics, structured tracing, health check RPC, Docker genesis).
Also own Tier 4 (atomic commits, block persistence, state pruning, snapshots).
Make the node production-ready for deployment. Fix the docker-compose.test.yml to actually work."
            ;;
        *)
            ROLE="General Engineer"
            FOCUS="Pick the highest-priority uncompleted task from any tier. Check PROGRESS.md and gh pr list first."
            ;;
    esac

    cat <<PROMPT
${TASKS}

---
## Your Role: ${ROLE} (Agent ${AGENT_ID} of ${TOTAL})

${FOCUS}

## Coordination Protocol

You are part of a ${TOTAL}-agent engineering team working in parallel. Each agent has its own git worktree so there are NO race conditions on files.

**Before starting work:**
1. Read PROGRESS.md to see what other agents have done
2. Run \`gh pr list --state all --limit 50\` to see open and merged PRs
3. Pick a task that NO other agent is working on (no open PR for it)

**Communication via PROGRESS.md:**
- Before starting: append "Agent ${AGENT_ID} (${ROLE}): STARTING <task name>" with timestamp
- After finishing: append "Agent ${AGENT_ID} (${ROLE}): COMPLETED <task name> — PR #N" with timestamp
- If blocked: append "Agent ${AGENT_ID} (${ROLE}): BLOCKED on <reason>"

**Branch naming:** fix/agent${AGENT_ID}-<scope>-<description>

**After each fix:**
1. Run \`cargo test --workspace --all-features\`
2. Run \`cargo clippy --all-targets --all-features -- -D warnings\`
3. Commit with conventional commit format
4. \`gh pr create\` then \`gh pr merge --squash --delete-branch\`
5. Update PROGRESS.md

**Working directory:** ${WORK_DIR}
PROMPT
}

# ── Run a single agent loop ──
run_agent() {
    local AGENT_ID=$1
    local TOTAL=$2
    local WORK_DIR=$3
    local LOCK_FILE="/tmp/claude-runner-agent${AGENT_ID}.lock"
    local RUNNER_LOG="${LOG_DIR}/runner-agent${AGENT_ID}.log"

    echo $$ > "$LOCK_FILE"

    echo "=== Agent $AGENT_ID ===" | tee "$RUNNER_LOG"
    echo "Start:    $(date -Iseconds)" | tee -a "$RUNNER_LOG"
    echo "Work dir: $WORK_DIR" | tee -a "$RUNNER_LOG"
    echo "=======================" | tee -a "$RUNNER_LOG"

    cd "$WORK_DIR"
    local CYCLE=0

    while true; do
        local NOW
        NOW=$(date +%s)
        [ "$NOW" -ge "$DEADLINE" ] && { echo "[$(date -Iseconds)] Agent $AGENT_ID: Time limit. Done." | tee -a "$RUNNER_LOG"; break; }
        [ -f "$STOP_FILE" ] && { echo "[$(date -Iseconds)] Agent $AGENT_ID: Stop file. Done." | tee -a "$RUNNER_LOG"; break; }

        CYCLE=$((CYCLE + 1))
        local LOG_FILE="${LOG_DIR}/agent${AGENT_ID}-cycle${CYCLE}-$(date +%Y%m%d-%H%M%S).log"

        echo "" | tee -a "$RUNNER_LOG"
        echo "[$(date -Iseconds)] Agent $AGENT_ID: Cycle $CYCLE" | tee -a "$RUNNER_LOG"

        # Reset to latest main
        git checkout main 2>/dev/null || true
        git pull --ff-only origin main 2>/dev/null || true

        # Build prompt
        local TASK_PROMPT
        TASK_PROMPT=$(agent_prompt "$AGENT_ID" "$TOTAL" "$WORK_DIR")

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Running claude → $LOG_FILE" | tee -a "$RUNNER_LOG"

        # Run claude
        caffeinate -dims \
            claude \
                --permission-mode bypassPermissions \
                --model claude-opus-4-6 \
                -p "$TASK_PROMPT" \
            >> "$LOG_FILE" 2>&1
        local EXIT_CODE=$?

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Exit $EXIT_CODE" | tee -a "$RUNNER_LOG"

        # Rate limit detection
        if [ "$EXIT_CODE" -ne 0 ] && grep -qi 'rate.limit\|429\|overloaded\|capacity\|quota' "$LOG_FILE" 2>/dev/null; then
            echo "[$(date -Iseconds)] Agent $AGENT_ID: Rate limited → waiting ${RATE_WAIT}s" | tee -a "$RUNNER_LOG"
            sleep "$RATE_WAIT"
            continue
        fi

        # Notify
        osascript -e "display notification \"Agent $AGENT_ID cycle $CYCLE (exit $EXIT_CODE)\" with title \"Aether\"" 2>/dev/null || true

        # Cooldown
        echo "[$(date -Iseconds)] Agent $AGENT_ID: Cooldown ${COOLDOWN}s" | tee -a "$RUNNER_LOG"
        sleep "$COOLDOWN"
    done

    rm -f "$LOCK_FILE"
    echo "[$(date -Iseconds)] Agent $AGENT_ID: Finished ($CYCLE cycles)" | tee -a "$RUNNER_LOG"
}

# ── Main: set up worktrees and launch agents ──
echo "=== Aether Engineering Team ===" | tee "${LOG_DIR}/runner.log"
echo "Agents:   $AGENTS" | tee -a "${LOG_DIR}/runner.log"
echo "Hours:    $MAX_HOURS" | tee -a "${LOG_DIR}/runner.log"
echo "Start:    $(date -Iseconds)" | tee -a "${LOG_DIR}/runner.log"
echo "================================" | tee -a "${LOG_DIR}/runner.log"

# Prune stale worktrees
git worktree prune 2>/dev/null || true

# Create worktrees for agents 2+
for i in $(seq 2 "$AGENTS"); do
    WT="/tmp/aether-agent${i}"
    if [ -d "$WT" ]; then
        git worktree remove --force "$WT" 2>/dev/null || rm -rf "$WT"
    fi
    git worktree prune 2>/dev/null || true
    echo "Creating worktree for agent $i → $WT" | tee -a "${LOG_DIR}/runner.log"
    git worktree add "$WT" main 2>&1 | tee -a "${LOG_DIR}/runner.log"
done

# Launch agents 2+ in background
for i in $(seq 2 "$AGENTS"); do
    WT="/tmp/aether-agent${i}"
    run_agent "$i" "$AGENTS" "$WT" &
    echo "Agent $i launched (PID $!)" | tee -a "${LOG_DIR}/runner.log"
done

# Agent 1 runs in foreground (main repo dir)
run_agent 1 "$AGENTS" "$REPO_DIR"

# Wait for all background agents
wait
echo "[$(date -Iseconds)] All agents complete." | tee -a "${LOG_DIR}/runner.log"

# Cleanup worktrees
for i in $(seq 2 "$AGENTS"); do
    git worktree remove --force "/tmp/aether-agent${i}" 2>/dev/null || true
done
git worktree prune 2>/dev/null || true
