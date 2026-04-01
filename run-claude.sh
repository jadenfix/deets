#!/usr/bin/env bash
# ============================================================================
# run-claude.sh — Autonomous engineering team for Aether blockchain
# ============================================================================
# Usage:
#   AGENTS=4 ./run-claude.sh           # Default: 4-agent team
#   AGENTS=2 ./run-claude.sh           # Custom count
#
# Environment:
#   MAX_HOURS=10      Max runtime (default 10)
#   COOLDOWN=60       Seconds between cycles (default 60)
#   AGENTS=4          Number of agents (default 4)
#   RATE_WAIT=300     Initial rate limit backoff (default 300s)
#   STAGGER=30        Seconds between agent launches (default 30)
#
# Kill switch:   touch /tmp/claude-runner-stop
# ============================================================================

set -uo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="${REPO_DIR}/.claude/logs"
STOP_FILE="/tmp/claude-runner-stop"
MAX_HOURS="${MAX_HOURS:-10}"
COOLDOWN="${COOLDOWN:-60}"
AGENTS="${AGENTS:-4}"
RATE_WAIT="${RATE_WAIT:-300}"
STAGGER="${STAGGER:-30}"
CODEX="/Users/jadenfix/Library/pnpm/nodejs/22.12.0/bin/codex"

COMMS_DIR="/tmp/aether-comms"
mkdir -p "$COMMS_DIR" "$LOG_DIR"
rm -f "$STOP_FILE"

export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

START_EPOCH=$(date +%s)
MAX_SECONDS=$(awk "BEGIN {printf \"%d\", $MAX_HOURS * 3600}")
DEADLINE=$((START_EPOCH + MAX_SECONDS))

# ══════════════════════════════════════════════════════════════════
# AGENT CONFIGURATION
# ══════════════════════════════════════════════════════════════════

AGENT_1_ROLE="Senior Engineer — Correctness, Safety & Consensus"
AGENT_1_MODEL="claude-opus-4-6"
AGENT_1_FOCUS="You are the team lead. Methodical, precise, deeply paranoid about correctness.

**Your expertise:** Cryptographic verification, state machine invariants, BFT protocols, VRF, BLS, finality.
**You own:** Tier 1 (tx signatures, double-spend, block validation, overflow, nonce, WASM gas) + Tier 2 (HotStuff liveness, slashing, fork choice, epochs, finality) + Tier 4 (storage atomicity, persistence).
**Style:** You audit code line-by-line. You think about adversarial inputs. You verify invariants formally. You add thorough edge-case tests."

AGENT_2_ROLE="Full-Stack Blockchain Engineer"
AGENT_2_MODEL="claude-opus-4-6"
AGENT_2_FOCUS="You are a versatile systems engineer who works across the entire stack.

**Your expertise:** Networking, P2P, libp2p, gossipsub, QUIC, state sync, Docker, CI/CD, build systems, Prometheus, tracing.
**You own:** Tier 3 (state sync, peer banning, message limits, graceful shutdown, backpressure) + Tier 6 (metrics, tracing, health checks, Docker, Dockerfile optimization, CI pipeline, dependency auditing).
**Style:** You think about failure modes, resource limits, and production readiness. You make things run reliably."

AGENT_3_ROLE="PR Reviewer & Quality Lead"
AGENT_3_MODEL="claude-opus-4-6"
AGENT_3_FOCUS="You are the team's code reviewer and quality gatekeeper. Your PRIMARY job is reviewing other agents' PRs before they merge. Your SECONDARY job is writing tests and benchmarks.

**Your workflow every cycle:**
1. Run \`gh pr list --state open\` to find unreviewed PRs
2. For each open PR:
   - Read the diff: \`gh pr diff <N>\`
   - Review for: correctness, edge cases, test coverage, naming, style
   - If good: \`gh pr review <N> --approve -b 'LGTM — <brief reason>'\` then \`gh pr merge <N> --squash --delete-branch\`
   - If issues: \`gh pr review <N> --request-changes -b '<specific feedback>'\`
   - Post to comms: \`echo '[timestamp] Agent 3: Reviewed PR #N — approved/changes requested' >> ${COMMS_DIR}/reviews.log\`
3. ONLY after all open PRs are reviewed, pick a task from Tier 5 (tests, proptests, benchmarks)

**Your expertise:** Code review, property testing, fuzzing, integration tests, benchmarks.
**You own:** Tier 5 (multi-node tests, proptests, Byzantine tests, benchmarks).
**Style:** You are constructive but thorough. You catch bugs others miss. You demand tests for every change."

AGENT_4_ROLE="Mid-Level Engineer"
AGENT_4_MODEL="claude-sonnet-4-6"
AGENT_4_FOCUS="You are a fast-moving, reliable engineer who ships well-scoped changes across any tier. You're not limited to cleanup — you take on real fixes and features, as long as they're clearly defined.

**Your expertise:** Clippy fixes, doc comments, refactoring, missing derives, dependency updates, BUT ALSO: implementing well-defined fixes from any tier, adding tests, wiring up features that have clear specs.
**You own:**
- Fix clippy warnings and compiler issues across the workspace
- Add missing doc comments to public APIs
- Clean up dead code, unused variables, redundant clones
- Add missing derives (Debug, Clone, Serialize/Deserialize)
- Pick up any well-scoped task from Tiers 1-6 that isn't claimed by another agent
- Implement straightforward features or fixes that other agents haven't gotten to
**Style:** You move fast. You ship clean, focused PRs. You don't overthink architecture — you execute."

# ══════════════════════════════════════════════════════════════════

get_agent_role() { eval echo "\${AGENT_${1}_ROLE:-General Engineer}"; }
get_agent_model() { eval echo "\${AGENT_${1}_MODEL:-claude-opus-4-6}"; }
get_agent_focus() { eval echo "\${AGENT_${1}_FOCUS:-Pick the highest-priority uncompleted task.}"; }

# ── Comms helpers ──
init_comms() {
    echo "# Agent Communication Board" > "$COMMS_DIR/general.log"
    echo "# PR Review Requests & Results" > "$COMMS_DIR/reviews.log"
    echo "# Task Claims" > "$COMMS_DIR/claims.log"
    echo "# Completed Work" > "$COMMS_DIR/completed.log"
    echo "# Blockers" > "$COMMS_DIR/blockers.log"
}

comms_post() {
    local CHANNEL="$1" MSG="$2"
    local FILE="$COMMS_DIR/${CHANNEL}.log"
    local TMP="$COMMS_DIR/.tmp.$$.$RANDOM"
    {
        cat "$FILE" 2>/dev/null
        echo "[$(date -Iseconds)] $MSG"
    } > "$TMP" && mv "$TMP" "$FILE"
}

# ── Build agent prompt ──
agent_prompt() {
    local AGENT_ID=$1 TOTAL=$2 WORK_DIR=$3
    local TASKS ROLE FOCUS CLAIMS COMPLETED GENERAL REVIEWS
    TASKS=$(cat "${REPO_DIR}/TASKS.md")
    ROLE=$(get_agent_role "$AGENT_ID")
    FOCUS=$(get_agent_focus "$AGENT_ID")
    CLAIMS=$(cat "$COMMS_DIR/claims.log" 2>/dev/null || echo "(empty)")
    COMPLETED=$(cat "$COMMS_DIR/completed.log" 2>/dev/null || echo "(empty)")
    GENERAL=$(tail -20 "$COMMS_DIR/general.log" 2>/dev/null || echo "(empty)")
    REVIEWS=$(tail -10 "$COMMS_DIR/reviews.log" 2>/dev/null || echo "(empty)")

    cat <<PROMPT
${TASKS}

---
## Your Role: ${ROLE} (Agent ${AGENT_ID} of ${TOTAL})

${FOCUS}

## Live Team Status

**Tasks claimed (DO NOT work on these):**
${CLAIMS}

**Completed by team:**
${COMPLETED}

**Recent reviews:**
${REVIEWS}

**Team chat:**
${GENERAL}

## Coordination

You are part of a ${TOTAL}-agent team. Each agent has its own git worktree — NO file conflicts.

**Before starting:**
1. Run \`gh pr list --state all --limit 50\`
2. Check claims above — don't duplicate work
3. Claim: \`echo "[\$(date -Iseconds)] Agent ${AGENT_ID} (${ROLE}): CLAIMING <task>" >> ${COMMS_DIR}/claims.log\`

**After finishing:**
1. \`echo "[\$(date -Iseconds)] Agent ${AGENT_ID} (${ROLE}): DONE <task> — PR #N" >> ${COMMS_DIR}/completed.log\`
2. \`echo "[\$(date -Iseconds)] Agent ${AGENT_ID}: <summary>" >> ${COMMS_DIR}/general.log\`

**Communication (instant, no git):**
- Chat: \`>> ${COMMS_DIR}/general.log\`
- Request review: \`>> ${COMMS_DIR}/reviews.log\`
- Flag blocker: \`>> ${COMMS_DIR}/blockers.log\`

**PR workflow:**
1. Branch: \`fix/agent${AGENT_ID}-<scope>-<description>\`
2. Code + \`cargo test --workspace --all-features\` + \`cargo clippy --all-targets --all-features -- -D warnings\`
3. \`git fetch origin main && git rebase origin/main\` before push
4. \`gh pr create\` with signature: \`🤖 Agent ${AGENT_ID} — ${ROLE}\`
5. Post to reviews.log asking Agent 3 to review (unless you ARE Agent 3)
6. If Agent 3 hasn't reviewed within this cycle, self-merge: \`gh pr merge --squash --delete-branch\`

**Working directory:** ${WORK_DIR}
PROMPT
}

# ── Run a single agent loop ──
run_agent() {
    local AGENT_ID=$1 TOTAL=$2 WORK_DIR=$3
    local LOCK_FILE="/tmp/claude-runner-agent${AGENT_ID}.lock"
    local RUNNER_LOG="${LOG_DIR}/runner-agent${AGENT_ID}.log"
    local MODEL
    MODEL=$(get_agent_model "$AGENT_ID")
    local ROLE
    ROLE=$(get_agent_role "$AGENT_ID")

    echo $$ > "$LOCK_FILE"

    echo "=== Agent $AGENT_ID ($ROLE) ===" | tee "$RUNNER_LOG"
    echo "Model:    $MODEL" | tee -a "$RUNNER_LOG"
    echo "Work dir: $WORK_DIR" | tee -a "$RUNNER_LOG"
    echo "Start:    $(date -Iseconds)" | tee -a "$RUNNER_LOG"
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

        # ── Sync to latest main ──
        echo "[$(date -Iseconds)] Agent $AGENT_ID: Syncing to latest main..." | tee -a "$RUNNER_LOG"
        git fetch origin main 2>&1 | tee -a "$RUNNER_LOG" || true
        git checkout --detach origin/main 2>&1 | tee -a "$RUNNER_LOG" || true

        # ── Build prompt with live comms ──
        local TASK_PROMPT
        TASK_PROMPT=$(agent_prompt "$AGENT_ID" "$TOTAL" "$WORK_DIR")

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Running $MODEL → $LOG_FILE" | tee -a "$RUNNER_LOG"

        local EXIT_CODE=0
        caffeinate -dims \
            claude \
                --permission-mode bypassPermissions \
                --model "$MODEL" \
                -p "$TASK_PROMPT" \
            >> "$LOG_FILE" 2>&1 || EXIT_CODE=$?

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Claude exit $EXIT_CODE" | tee -a "$RUNNER_LOG"

        # ── Rate limit: fall back to Codex, then exponential backoff ──
        if [ "$EXIT_CODE" -ne 0 ] && grep -qi 'rate.limit\|429\|overloaded\|capacity\|quota' "$LOG_FILE" 2>/dev/null; then
            echo "[$(date -Iseconds)] Agent $AGENT_ID: Claude rate limited → trying Codex" | tee -a "$RUNNER_LOG"
            comms_post "general" "Agent $AGENT_ID: Claude rate limited, switching to Codex"

            if [ -x "$CODEX" ]; then
                local CODEX_LOG="${LOG_FILE%.log}-codex.log"
                local CODEX_EXIT=0
                caffeinate -dims \
                    "$CODEX" exec \
                        --dangerously-bypass-approvals-and-sandbox \
                        -C "$WORK_DIR" \
                        "$TASK_PROMPT" \
                    < /dev/null >> "$CODEX_LOG" 2>&1 || CODEX_EXIT=$?
                echo "[$(date -Iseconds)] Agent $AGENT_ID: Codex exit $CODEX_EXIT" | tee -a "$RUNNER_LOG"

                if [ "$CODEX_EXIT" -ne 0 ] && grep -qi 'rate.limit\|429\|overloaded\|capacity\|quota' "$CODEX_LOG" 2>/dev/null; then
                    local BACKOFF=${RATE_WAIT}
                    local RATE_FILE="$COMMS_DIR/.rate_limited.agent${AGENT_ID}"
                    if [ -f "$RATE_FILE" ]; then
                        local PREV
                        PREV=$(cat "$RATE_FILE")
                        BACKOFF=$((PREV * 2))
                        [ "$BACKOFF" -gt 1800 ] && BACKOFF=1800
                    fi
                    echo "$BACKOFF" > "$RATE_FILE"
                    local JITTER=$(( (RANDOM % (BACKOFF / 5 + 1)) - BACKOFF / 10 ))
                    BACKOFF=$((BACKOFF + JITTER))
                    echo "[$(date -Iseconds)] Agent $AGENT_ID: Both rate limited → ${BACKOFF}s backoff" | tee -a "$RUNNER_LOG"
                    comms_post "general" "Agent $AGENT_ID: Both Claude+Codex rate limited, sleeping ${BACKOFF}s"
                    sleep "$BACKOFF"
                else
                    rm -f "$COMMS_DIR/.rate_limited.agent${AGENT_ID}"
                fi
            else
                echo "[$(date -Iseconds)] Agent $AGENT_ID: Codex not found, backoff ${RATE_WAIT}s" | tee -a "$RUNNER_LOG"
                sleep "$RATE_WAIT"
            fi
            continue
        else
            rm -f "$COMMS_DIR/.rate_limited.agent${AGENT_ID}"
        fi

        osascript -e "display notification \"Agent $AGENT_ID cycle $CYCLE (exit $EXIT_CODE)\" with title \"Aether\"" 2>/dev/null || true

        echo "[$(date -Iseconds)] Agent $AGENT_ID: Cooldown ${COOLDOWN}s" | tee -a "$RUNNER_LOG"
        sleep "$COOLDOWN"
    done

    rm -f "$LOCK_FILE"
    comms_post "general" "Agent $AGENT_ID: Shutting down after $CYCLE cycles"
    echo "[$(date -Iseconds)] Agent $AGENT_ID: Finished ($CYCLE cycles)" | tee -a "$RUNNER_LOG"
}

# ══════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════

echo "=== Aether Engineering Team ===" | tee "${LOG_DIR}/runner.log"
echo "Agents: $AGENTS | Hours: $MAX_HOURS | Stagger: ${STAGGER}s | Cooldown: ${COOLDOWN}s" | tee -a "${LOG_DIR}/runner.log"
echo "Start:  $(date -Iseconds)" | tee -a "${LOG_DIR}/runner.log"
for i in $(seq 1 "$AGENTS"); do
    echo "  Agent $i: $(get_agent_role "$i") [$(get_agent_model "$i")]" | tee -a "${LOG_DIR}/runner.log"
done
echo "================================" | tee -a "${LOG_DIR}/runner.log"

init_comms
comms_post "general" "Runner: Launching $AGENTS agents"

git worktree prune 2>/dev/null || true

# Create worktrees for agents 2+
for i in $(seq 2 "$AGENTS"); do
    WT="/tmp/aether-agent${i}"
    [ -d "$WT" ] && { git worktree remove --force "$WT" 2>/dev/null || rm -rf "$WT"; }
    git worktree prune 2>/dev/null || true
    echo "Creating worktree for agent $i → $WT" | tee -a "${LOG_DIR}/runner.log"
    git worktree add --detach "$WT" HEAD 2>&1 | tee -a "${LOG_DIR}/runner.log"
done

# Launch agents 2+ with stagger delay
for i in $(seq 2 "$AGENTS"); do
    WT="/tmp/aether-agent${i}"
    run_agent "$i" "$AGENTS" "$WT" &
    echo "Agent $i launched (PID $!)" | tee -a "${LOG_DIR}/runner.log"
    comms_post "general" "Runner: Agent $i online ($(get_agent_role "$i"))"
    if [ "$i" -lt "$AGENTS" ]; then
        echo "Staggering ${STAGGER}s before next agent..." | tee -a "${LOG_DIR}/runner.log"
        sleep "$STAGGER"
    fi
done

# Agent 1 in foreground (after final stagger)
sleep "$STAGGER"
comms_post "general" "Runner: Agent 1 online ($(get_agent_role 1))"
run_agent 1 "$AGENTS" "$REPO_DIR"

wait
echo "[$(date -Iseconds)] All agents complete." | tee -a "${LOG_DIR}/runner.log"

for i in $(seq 2 "$AGENTS"); do
    git worktree remove --force "/tmp/aether-agent${i}" 2>/dev/null || true
done
git worktree prune 2>/dev/null || true
