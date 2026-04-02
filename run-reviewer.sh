#!/usr/bin/env bash
# ============================================================================
# run-reviewer.sh — Dedicated PR reviewer agent for Aether blockchain
# ============================================================================
# A single Opus 4.6 agent that ONLY reviews, fixes, and merges PRs.
# Runs independently of run-claude.sh. Communicates via /tmp/aether-comms/.
#
# Usage:
#   ./run-reviewer.sh                # Run until MAX_HOURS
#   MAX_HOURS=8 ./run-reviewer.sh    # Custom duration
#
# Kill switch:   touch /tmp/claude-reviewer-stop
# ============================================================================

set -uo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="${REPO_DIR}/.claude/logs"
LOCK_FILE="/tmp/claude-reviewer.lock"
STOP_FILE="/tmp/claude-reviewer-stop"
MAX_HOURS="${MAX_HOURS:-10}"
COOLDOWN="${COOLDOWN:-30}"
RATE_WAIT="${RATE_WAIT:-300}"
CODEX="/Users/jadenfix/Library/pnpm/nodejs/22.12.0/bin/codex"
COMMS_DIR="/tmp/aether-comms"
WORKTREE="/tmp/aether-reviewer"

MODEL="claude-opus-4-6"
AGENT_NAME="Reviewer Agent"
AGENT_SIG="🔍 Reviewer Agent (Opus 4.6)"

mkdir -p "$COMMS_DIR" "$LOG_DIR"
rm -f "$STOP_FILE"

export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

# ── Guard ──
if [ -f "$LOCK_FILE" ]; then
    EXISTING_PID=$(cat "$LOCK_FILE" 2>/dev/null || echo "")
    if [ -n "$EXISTING_PID" ] && kill -0 "$EXISTING_PID" 2>/dev/null; then
        echo "ERROR: Reviewer already running (PID ${EXISTING_PID})" >&2
        exit 1
    fi
    rm -f "$LOCK_FILE"
fi
echo $$ > "$LOCK_FILE"

comms_post() {
    local CHANNEL="$1" MSG="$2"
    local FILE="$COMMS_DIR/${CHANNEL}.log"
    local TMP="$COMMS_DIR/.tmp.$$.$RANDOM"
    { cat "$FILE" 2>/dev/null; echo "[$(date -Iseconds)] $MSG"; } > "$TMP" && mv "$TMP" "$FILE"
}

setup_worktree() {
    if [ -d "$WORKTREE" ]; then
        git -C "$REPO_DIR" worktree remove --force "$WORKTREE" 2>/dev/null || rm -rf "$WORKTREE"
    fi
    git -C "$REPO_DIR" worktree prune 2>/dev/null || true
    git -C "$REPO_DIR" worktree add --detach "$WORKTREE" HEAD 2>&1
}

cleanup() {
    rm -f "$LOCK_FILE"
    comms_post "general" "$AGENT_NAME: Going offline."
    if [ -d "$WORKTREE" ]; then
        git -C "$REPO_DIR" worktree remove --force "$WORKTREE" 2>/dev/null || true
    fi
    git -C "$REPO_DIR" worktree prune 2>/dev/null || true
}
trap cleanup EXIT

reviewer_prompt() {
    local WORK_DIR=$1
    local CLAUDE_MD PROGRESS_MD GENERAL REVIEWS COMPLETED BLOCKERS
    CLAUDE_MD=$(cat "${REPO_DIR}/CLAUDE.md" 2>/dev/null || echo "(not found)")
    PROGRESS_MD=$(tail -50 "${REPO_DIR}/progress.md" 2>/dev/null || echo "(not found)")
    GENERAL=$(tail -30 "$COMMS_DIR/general.log" 2>/dev/null || echo "(none)")
    REVIEWS=$(tail -30 "$COMMS_DIR/reviews.log" 2>/dev/null || echo "(none)")
    COMPLETED=$(tail -20 "$COMMS_DIR/completed.log" 2>/dev/null || echo "(none)")
    BLOCKERS=$(cat "$COMMS_DIR/blockers.log" 2>/dev/null || echo "(none)")

    cat <<'PROMPT_START'
# You are the Dedicated PR Reviewer for the Aether Blockchain

You are 🔍 Reviewer Agent — a senior code reviewer running on Opus 4.6 with max effort.
You have ONE job: keep the codebase healthy by reviewing, fixing, and merging pull requests.

**You NEVER create feature PRs. You NEVER pick tasks from the tier list. You only review.**

## Your Identity
- Signature on reviews: `Reviewed-By: Reviewer Agent (Opus 4.6) <reviewer@aether.dev>`
- Signature on commits: `Co-Authored-By: Reviewer Agent (Opus 4.6) <reviewer@aether.dev>`

## Your Cycle (do these in order)

### Step 1: Check CI Health
```bash
cargo test --workspace --all-features 2>&1 | tail -50
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -30
```
If ANYTHING fails: fix it on `fix/reviewer-ci-<issue>`, push, create PR, ask Agent 1 to merge.

### Step 2: Review Every Open PR
```bash
gh pr list --state open
```
For EACH open PR (skip any with 'reviewer' in the branch name):

a) Read: `gh pr view <N>` and `gh pr diff <N>`
b) Check context: what does this fix? does it match description?
c) Review for: correctness, safety (unchecked math, missing sig checks, panics), tests, style
d) Test: `git fetch origin && git checkout <branch> && cargo check --workspace 2>&1 | tail -20`

e) Decision:
- **GOOD**: `gh pr review <N> --approve -b "Reviewed-By: Reviewer Agent. LGTM — <reason>"` then `gh pr merge <N> --squash --delete-branch`
- **MINOR issues**: Fix it yourself (push commit to their branch), then approve and merge
- **MAJOR issues**: `gh pr review <N> --request-changes -b "<specific feedback with file:line refs>"`
- **Merge conflicts**: Checkout branch, rebase onto origin/main, force-push, then review

Post every decision to reviews.log.

### Step 3: Communicate
Post cycle summary to general.log. Read and respond to blockers.
PROMPT_START

    cat <<PROMPT_CONTEXT

## Project Context
${CLAUDE_MD}

## Team Activity
**Chat:** ${GENERAL}
**Reviews:** ${REVIEWS}
**Completed:** ${COMPLETED}
**Blockers:** ${BLOCKERS}
**Progress:** ${PROGRESS_MD}

**Working directory:** ${WORK_DIR}
PROMPT_CONTEXT
}

START_EPOCH=$(date +%s)
MAX_SECONDS=$(awk "BEGIN {printf \"%d\", $MAX_HOURS * 3600}")
DEADLINE=$((START_EPOCH + MAX_SECONDS))
RATE_FILE="$COMMS_DIR/.rate_limited.reviewer"
CONSECUTIVE_FAILURES=0

RUNNER_LOG="${LOG_DIR}/runner-reviewer.log"

echo "=== $AGENT_SIG ===" | tee "$RUNNER_LOG"
echo "Model:    $MODEL" | tee -a "$RUNNER_LOG"
echo "Cooldown: ${COOLDOWN}s" | tee -a "$RUNNER_LOG"
echo "Hours:    $MAX_HOURS" | tee -a "$RUNNER_LOG"
echo "Start:    $(date -Iseconds)" | tee -a "$RUNNER_LOG"
echo "=========================" | tee -a "$RUNNER_LOG"

setup_worktree 2>&1 | tee -a "$RUNNER_LOG"
comms_post "general" "$AGENT_NAME: Online. Reviewing PRs and guarding CI."

cd "$WORKTREE"
CYCLE=0

while true; do
    NOW=$(date +%s)
    [ "$NOW" -ge "$DEADLINE" ] && { echo "[$(date -Iseconds)] Reviewer: Time limit." | tee -a "$RUNNER_LOG"; break; }
    [ -f "$STOP_FILE" ] && { echo "[$(date -Iseconds)] Reviewer: Stop file." | tee -a "$RUNNER_LOG"; break; }

    [ "$CONSECUTIVE_FAILURES" -ge 5 ] && { comms_post "general" "$AGENT_NAME: Circuit breaker (5 failures). Sleeping 30 min."; sleep 1800; CONSECUTIVE_FAILURES=0; }

    CYCLE=$((CYCLE + 1))
    LOG_FILE="${LOG_DIR}/reviewer-cycle${CYCLE}-$(date +%Y%m%d-%H%M%S).log"

    echo "" | tee -a "$RUNNER_LOG"
    echo "[$(date -Iseconds)] Reviewer: Cycle $CYCLE" | tee -a "$RUNNER_LOG"

    # Ensure worktree exists (recreate if deleted)
    if ! cd "$WORKTREE" 2>/dev/null || ! pwd -P >/dev/null 2>&1; then
        echo "[$(date -Iseconds)] Reviewer: Worktree gone, recreating..." | tee -a "$RUNNER_LOG"
        cd "$REPO_DIR"
        git worktree remove --force "$WORKTREE" 2>/dev/null || true
        git worktree prune 2>/dev/null || true
        git worktree add --detach "$WORKTREE" HEAD 2>&1 | tee -a "$RUNNER_LOG"
        cd "$WORKTREE"
    fi

    git checkout -- . 2>/dev/null || true
    git clean -fd 2>/dev/null || true
    git -C "$WORKTREE" fetch origin main 2>&1 | tee -a "$RUNNER_LOG" || true
    git -C "$WORKTREE" checkout --detach origin/main 2>&1 | tee -a "$RUNNER_LOG" || true

    TASK_PROMPT=$(reviewer_prompt "$WORKTREE")

    echo "[$(date -Iseconds)] Reviewer: Running → $LOG_FILE" | tee -a "$RUNNER_LOG"

    EXIT_CODE=0
    caffeinate -dims claude --permission-mode bypassPermissions --model "$MODEL" -p "$TASK_PROMPT" >> "$LOG_FILE" 2>&1 || EXIT_CODE=$?

    echo "[$(date -Iseconds)] Reviewer: Exit $EXIT_CODE" | tee -a "$RUNNER_LOG"

    if [ "$EXIT_CODE" -eq 0 ]; then
        CONSECUTIVE_FAILURES=0; rm -f "$RATE_FILE"
    elif grep -qi 'rate.limit\|429\|overloaded\|capacity\|quota' "$LOG_FILE" 2>/dev/null; then
        comms_post "general" "$AGENT_NAME: Rate limited, trying Codex"
        if [ -x "$CODEX" ]; then
            CODEX_LOG="${LOG_FILE%.log}-codex.log"
            PROMPT_FILE="$COMMS_DIR/.prompt-reviewer.txt"
            echo "$TASK_PROMPT" > "$PROMPT_FILE"
            CODEX_EXIT=0
            caffeinate -dims "$CODEX" exec --dangerously-bypass-approvals-and-sandbox -c 'reasoning_effort="high"' -C "$WORKTREE" - < "$PROMPT_FILE" >> "$CODEX_LOG" 2>&1 || CODEX_EXIT=$?
            [ "$CODEX_EXIT" -eq 0 ] && { CONSECUTIVE_FAILURES=0; rm -f "$RATE_FILE"; } || {
                BACKOFF=${RATE_WAIT}; [ -f "$RATE_FILE" ] && BACKOFF=$(( $(cat "$RATE_FILE") * 2 )) && [ "$BACKOFF" -gt 1800 ] && BACKOFF=1800
                echo "$BACKOFF" > "$RATE_FILE"; sleep "$BACKOFF"; continue
            }
        else
            sleep "$RATE_WAIT"; continue
        fi
    else
        CONSECUTIVE_FAILURES=$((CONSECUTIVE_FAILURES + 1)); sleep 30; continue
    fi

    echo "[$(date -Iseconds)] Reviewer: Cooldown ${COOLDOWN}s" | tee -a "$RUNNER_LOG"
    sleep "$COOLDOWN"
done

echo "[$(date -Iseconds)] Reviewer: Finished ($CYCLE cycles)" | tee -a "$RUNNER_LOG"
