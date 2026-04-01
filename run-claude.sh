#!/usr/bin/env bash
# ============================================================================
# run-claude.sh — Autonomous engineering team for Aether blockchain
# ============================================================================
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

# ── Shared comms directory (filesystem-based, no git needed) ──
COMMS_DIR="/tmp/aether-comms"
mkdir -p "$COMMS_DIR"

mkdir -p "$LOG_DIR"
rm -f "$STOP_FILE"

export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

START_EPOCH=$(date +%s)
MAX_SECONDS=$(awk "BEGIN {printf \"%d\", $MAX_HOURS * 3600}")
DEADLINE=$((START_EPOCH + MAX_SECONDS))

# ── Initialize shared comms files ──
init_comms() {
    echo "# Agent Communication Board (live, filesystem-based)" > "$COMMS_DIR/general.log"
    echo "# PR Review Requests" > "$COMMS_DIR/reviews.log"
    echo "# Architecture Decisions" > "$COMMS_DIR/architecture.log"
    echo "# Blockers" > "$COMMS_DIR/blockers.log"
    echo "# Task Claims — agents write here before starting work" > "$COMMS_DIR/claims.log"
    echo "# Completed Work" > "$COMMS_DIR/completed.log"
}

# ── Comms helper: append a message (atomic via temp+mv) ──
comms_post() {
    local CHANNEL="$1"
    local MSG="$2"
    local FILE="$COMMS_DIR/${CHANNEL}.log"
    local TMP="$COMMS_DIR/.tmp.$$"
    {
        cat "$FILE" 2>/dev/null
        echo "[$(date -Iseconds)] $MSG"
    } > "$TMP" && mv "$TMP" "$FILE"
}

# ── Check if a task is already claimed ──
task_claimed() {
    local TASK_KEY="$1"
    grep -q "$TASK_KEY" "$COMMS_DIR/claims.log" 2>/dev/null || \
    grep -q "$TASK_KEY" "$COMMS_DIR/completed.log" 2>/dev/null
}

# ── Agent role definitions ──
agent_prompt() {
    local AGENT_ID=$1
    local TOTAL=$2
    local WORK_DIR=$3
    local TASKS
    TASKS=$(cat "${REPO_DIR}/TASKS.md")

    # Read current comms state
    local CLAIMS COMPLETED GENERAL
    CLAIMS=$(cat "$COMMS_DIR/claims.log" 2>/dev/null || echo "(empty)")
    COMPLETED=$(cat "$COMMS_DIR/completed.log" 2>/dev/null || echo "(empty)")
    GENERAL=$(tail -20 "$COMMS_DIR/general.log" 2>/dev/null || echo "(empty)")

    local ROLE=""
    local FOCUS=""

    case $AGENT_ID in
        1)
            ROLE="Architect — Correctness & Safety Lead"
            FOCUS="You are the team lead. Methodical, precise, deeply paranoid about correctness.

**Your expertise:** Cryptographic verification, formal correctness, state machine invariants.
**You own:** Tier 1 (tx signatures, double-spend, block validation, overflow, nonce, WASM gas).
**Then:** Tier 4 (storage atomicity, persistence).
**Style:** You audit code line-by-line. You think about adversarial inputs. You add edge-case tests."
            ;;
        2)
            ROLE="Consensus Protocol Specialist"
            FOCUS="You are the consensus expert. You think in terms of safety and liveness proofs.

**Your expertise:** BFT protocols, VRF, BLS aggregation, finality gadgets, fork choice rules.
**You own:** Tier 2 (HotStuff liveness, slashing, fork choice, epochs, finality).
**Then:** Help write Byzantine fault tests.
**Style:** You verify invariants formally. You think about n=3f+1."
            ;;
        3)
            ROLE="Networking & Systems Engineer"
            FOCUS="You are a systems programmer who thinks about failure modes and graceful degradation.

**Your expertise:** libp2p, gossipsub, QUIC, state sync, connection management.
**You own:** Tier 3 (state sync, peer banning, message limits, graceful shutdown, backpressure).
**Then:** Tier 6 (Docker networking, genesis ceremony).
**Style:** You think about what happens under load. You add rate limits."
            ;;
        4)
            ROLE="Quality & Testing Engineer"
            FOCUS="You are obsessed with test coverage and proving correctness through property-based testing.

**Your expertise:** proptest, fuzzing, integration testing, benchmarking.
**You own:** Tier 5 (multi-node tests, proptests, Byzantine tests, benchmarks).
**Style:** You generate random inputs. You test boundaries. You benchmark."
            ;;
        5)
            ROLE="Platform & Optimization Engineer"
            FOCUS="You are the pragmatic engineer who makes things run in production.

**Your expertise:** Prometheus, tracing, Docker, RocksDB tuning, memory profiling.
**You own:** Tier 6 (metrics, tracing, health checks, Docker) + Tier 4 (state pruning, snapshots).
**Style:** You add metrics to everything. You reduce allocations."
            ;;
        6)
            ROLE="CI/CD & DevOps Engineer"
            FOCUS="You are the build and release engineer. You understand every stage of a CI/CD pipeline deeply — from git hooks to container registries. You think about reproducibility, caching, and fast feedback loops.

**Your expertise:** GitHub Actions, Dockerfile optimization, cargo workspace builds, test parallelization, build caching, release automation, multi-stage builds, dependency auditing.
**You own:**
- Ensure \`cargo test --workspace --all-features\` passes on clean checkout
- Ensure \`cargo clippy --all-targets --all-features -- -D warnings\` is zero warnings
- Optimize Dockerfile for layer caching and small images
- Fix docker-compose.test.yml so the 4-node test network actually boots and passes health checks
- Add a Makefile or justfile target for every common operation
- Audit dependencies with \`cargo audit\` and \`cargo deny\`
- Ensure the build is reproducible (pinned deps, deterministic features)
**Style:** You run the full build from scratch. You time it. You make it faster. You make it never break."
            ;;
        7)
            ROLE="Junior Engineer (Sonnet)"
            FOCUS="You handle the straightforward, well-defined tasks that don't require deep architecture knowledge. You're fast and reliable.

**Your expertise:** Clippy fixes, doc comments, code formatting, simple refactors, adding missing derives, fixing compiler warnings, updating dependencies.
**You own:**
- Fix any remaining clippy warnings across the workspace
- Add missing doc comments to public APIs
- Fix any broken or outdated imports
- Clean up dead code, unused variables, redundant clones
- Add missing \`Debug\`, \`Clone\`, \`Serialize\`/\`Deserialize\` derives where needed
- Update Cargo.toml metadata (descriptions, categories, keywords)
- Fix any TODO comments that have obvious solutions
**Style:** You make small, clean PRs. You don't overthink. You ship fast."
            ;;
        *)
            ROLE="General Engineer"
            FOCUS="Pick the highest-priority uncompleted task from any tier."
            ;;
    esac

    cat <<PROMPT
${TASKS}

---
## Your Role: ${ROLE} (Agent ${AGENT_ID} of ${TOTAL})

${FOCUS}

## Live Team Status

**Tasks claimed by other agents (DO NOT work on these):**
${CLAIMS}

**Tasks completed by the team:**
${COMPLETED}

**Recent team chat:**
${GENERAL}

## Coordination Protocol

You are part of a ${TOTAL}-agent team. Each agent has its own git worktree — NO file conflicts.

**Before starting work:**
1. Run \`gh pr list --state all --limit 50\` to see what's done/in-flight
2. Read the team status above — do NOT pick a task that's already claimed or completed
3. Claim your task: \`echo "[$(date -Iseconds)] Agent ${AGENT_ID} (${ROLE}): CLAIMING <task>" >> ${COMMS_DIR}/claims.log\`

**After finishing:**
1. Post completion: \`echo "[$(date -Iseconds)] Agent ${AGENT_ID} (${ROLE}): DONE <task> — PR #N" >> ${COMMS_DIR}/completed.log\`
2. Post to general: \`echo "[$(date -Iseconds)] Agent ${AGENT_ID}: <summary of what you did>" >> ${COMMS_DIR}/general.log\`

**Communication (real-time, no git needed):**
- General chat: \`echo "msg" >> ${COMMS_DIR}/general.log\`
- Request review: \`echo "msg" >> ${COMMS_DIR}/reviews.log\`
- Flag blocker: \`echo "msg" >> ${COMMS_DIR}/blockers.log\`
- Read any channel: \`cat ${COMMS_DIR}/<channel>.log\`

**PR workflow:**
1. Branch: \`fix/agent${AGENT_ID}-<scope>-<description>\`
2. Code + test: \`cargo test --workspace --all-features\` + \`cargo clippy\`
3. Before push: \`git fetch origin main && git rebase origin/main\` (resolve conflicts if any)
4. Commit with conventional format
5. \`gh pr create\` with signature: \`🤖 Agent ${AGENT_ID} — ${ROLE}\`
6. Self-merge: \`gh pr merge --squash --delete-branch\`

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
    echo "Comms:    $COMMS_DIR" | tee -a "$RUNNER_LOG"
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

        # ── SYNC: Always pull latest main before each cycle ──
        echo "[$(date -Iseconds)] Agent $AGENT_ID: Syncing to latest main..." | tee -a "$RUNNER_LOG"
        git fetch origin main 2>&1 | tee -a "$RUNNER_LOG" || true
        git checkout --detach origin/main 2>&1 | tee -a "$RUNNER_LOG" || true

        # ── Build prompt with live comms state ──
        local TASK_PROMPT
        TASK_PROMPT=$(agent_prompt "$AGENT_ID" "$TOTAL" "$WORK_DIR")

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Running claude → $LOG_FILE" | tee -a "$RUNNER_LOG"

        # ── Pick model (Agent 7 uses Sonnet, rest use Opus) ──
        local MODEL="claude-opus-4-6"
        if [ "$AGENT_ID" -eq 7 ]; then
            MODEL="claude-sonnet-4-6"
        fi

        # ── Run claude ──
        caffeinate -dims \
            claude \
                --permission-mode bypassPermissions \
                --model "$MODEL" \
                -p "$TASK_PROMPT" \
            >> "$LOG_FILE" 2>&1
        local EXIT_CODE=$?

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Exit $EXIT_CODE" | tee -a "$RUNNER_LOG"

        # ── Rate limit detection ──
        if [ "$EXIT_CODE" -ne 0 ] && grep -qi 'rate.limit\|429\|overloaded\|capacity\|quota' "$LOG_FILE" 2>/dev/null; then
            echo "[$(date -Iseconds)] Agent $AGENT_ID: Rate limited → waiting ${RATE_WAIT}s" | tee -a "$RUNNER_LOG"
            comms_post "general" "Agent $AGENT_ID: Rate limited, backing off ${RATE_WAIT}s"
            sleep "$RATE_WAIT"
            continue
        fi

        osascript -e "display notification \"Agent $AGENT_ID cycle $CYCLE (exit $EXIT_CODE)\" with title \"Aether\"" 2>/dev/null || true

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Cooldown ${COOLDOWN}s" | tee -a "$RUNNER_LOG"
        sleep "$COOLDOWN"
    done

    rm -f "$LOCK_FILE"
    echo "[$(date -Iseconds)] Agent $AGENT_ID: Finished ($CYCLE cycles)" | tee -a "$RUNNER_LOG"
}

# ══════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════

echo "=== Aether Engineering Team ===" | tee "${LOG_DIR}/runner.log"
echo "Agents: $AGENTS | Hours: $MAX_HOURS | Comms: $COMMS_DIR" | tee -a "${LOG_DIR}/runner.log"
echo "Start:  $(date -Iseconds)" | tee -a "${LOG_DIR}/runner.log"
echo "================================" | tee -a "${LOG_DIR}/runner.log"

# Initialize shared comms
init_comms
comms_post "general" "Runner: Launching $AGENTS agents"

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
    git worktree add --detach "$WT" HEAD 2>&1 | tee -a "${LOG_DIR}/runner.log"
done

# Launch agents 2+ in background
for i in $(seq 2 "$AGENTS"); do
    WT="/tmp/aether-agent${i}"
    run_agent "$i" "$AGENTS" "$WT" &
    echo "Agent $i launched (PID $!)" | tee -a "${LOG_DIR}/runner.log"
    comms_post "general" "Runner: Agent $i online"
done

# Agent 1 runs in foreground
run_agent 1 "$AGENTS" "$REPO_DIR"

wait
echo "[$(date -Iseconds)] All agents complete." | tee -a "${LOG_DIR}/runner.log"

# Cleanup
for i in $(seq 2 "$AGENTS"); do
    git worktree remove --force "/tmp/aether-agent${i}" 2>/dev/null || true
done
git worktree prune 2>/dev/null || true
