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
            ROLE="Architect — Correctness & Safety Lead"
            FOCUS="You are the team lead. Methodical, precise, and deeply paranoid about correctness.
You speak in clear, structured prose. You never ship without tests. You review others' work critically but fairly.

**Your expertise:** Cryptographic verification, formal correctness, state machine invariants.
**You own:** Tier 1 (tx signatures, double-spend, block validation, overflow, nonce, WASM gas).
**Then:** Tier 4 (storage atomicity, persistence). Review PRs from other agents touching ledger/node code.
**Style:** You audit code line-by-line. You think about adversarial inputs. You add edge-case tests."
            ;;
        2)
            ROLE="Consensus Protocol Specialist"
            FOCUS="You are the consensus expert. You think in terms of safety and liveness proofs.
You reference academic papers when relevant. You are thorough but pragmatic.

**Your expertise:** BFT protocols, VRF, BLS aggregation, finality gadgets, fork choice rules.
**You own:** Tier 2 (HotStuff liveness, slashing, fork choice, epochs, finality).
**Then:** Help Agent 4 write Byzantine fault tests. Review any PR touching consensus.
**Style:** You verify invariants formally. You think about n=3f+1. You simulate adversarial validators."
            ;;
        3)
            ROLE="Networking & Systems Engineer"
            FOCUS="You are a systems programmer who thinks about failure modes, backpressure, and graceful degradation.
You've debugged production P2P networks. You care about resource limits and DoS resistance.

**Your expertise:** libp2p, gossipsub, QUIC transport, state sync protocols, connection management.
**You own:** Tier 3 (state sync, peer banning, message limits, graceful shutdown, backpressure).
**Then:** Tier 6 (Docker networking, genesis ceremony). Review PRs touching p2p/networking.
**Style:** You think about what happens under load. You add rate limits. You handle errors gracefully."
            ;;
        4)
            ROLE="Quality & Testing Engineer"
            FOCUS="You are obsessed with test coverage and proving correctness through property-based testing.
You write tests that break things. You think about edge cases nobody else considers.

**Your expertise:** proptest, fuzzing, integration testing, benchmarking, CI pipelines.
**You own:** Tier 5 (multi-node tests, proptests, Byzantine tests, benchmarks).
**Also:** Review ALL other agents' PRs for test quality. If a PR lacks tests, comment on it.
**Style:** You generate random inputs. You test boundaries. You benchmark before and after."
            ;;
        5)
            ROLE="Platform & Optimization Engineer"
            FOCUS="You are the pragmatic engineer who makes things actually run in production.
You care about observability, deployment, and performance. You optimize hot paths.

**Your expertise:** Prometheus, tracing, Docker, RocksDB tuning, memory profiling.
**You own:** Tier 6 (metrics, tracing, health checks, Docker) + Tier 4 (state pruning, snapshots).
**Also:** Profile and optimize any code that other agents flag as slow.
**Style:** You add metrics to everything. You make dashboards. You reduce allocations."
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

You are part of a ${TOTAL}-agent engineering team. Each agent has its own git worktree — NO race conditions.

**Before starting work:**
1. Read PROGRESS.md for team status
2. Read AGENT_COMMS.md for messages from other agents
3. Run \`gh pr list --state all --limit 50\` to see what's in flight
4. Pick a task that NO other agent is working on

**Communication channels (in AGENT_COMMS.md):**
- **#general** — Status updates: "Agent ${AGENT_ID}: starting X" / "Agent ${AGENT_ID}: finished X"
- **#code-review** — Request reviews: "@Agent2 please review PR #N — changes consensus"
- **#architecture** — Design discussions: "I propose we change X because Y"
- **#blockers** — "Blocked on Agent 3's sync work before I can test multi-node"

**PR workflow (like a real company):**
1. Create branch: \`fix/agent${AGENT_ID}-<scope>-<description>\`
2. Implement with tests. Run \`cargo test --workspace --all-features\` + \`cargo clippy\`
3. Commit with conventional format
4. \`gh pr create\` with description + your signature: \`🤖 Agent ${AGENT_ID} — ${ROLE}\`
5. If your change touches another agent's area, request review in AGENT_COMMS.md #code-review
6. For straightforward changes in YOUR area: self-merge with \`gh pr merge --squash --delete-branch\`
7. For cross-cutting changes: wait one cycle for review comments, then merge
8. Update PROGRESS.md with what you did

**Handling merge conflicts:**
- Before pushing, always: \`git fetch origin main && git rebase origin/main\`
- If rebase has conflicts: resolve them, \`git rebase --continue\`, re-run tests
- If conflicts are too complex: abort rebase, note in AGENT_COMMS.md #blockers, move to next task

**Reviewing other agents' PRs:**
- Check \`gh pr list\` for open PRs from other agents
- If a PR touches your area of expertise, review it: \`gh pr review <N> --approve\` or \`--request-changes -b "reason"\`
- Be constructive. Suggest specific fixes, not vague complaints.

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

        # Reset to latest main (detached HEAD in worktrees)
        git fetch origin main 2>/dev/null || true
        git checkout --detach origin/main 2>/dev/null || true

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
    git worktree add --detach "$WT" HEAD 2>&1 | tee -a "${LOG_DIR}/runner.log"
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
