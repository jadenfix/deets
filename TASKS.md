# Aether Blockchain — Engineering Team Mission

## 🚀 Efficiency + Quality Rules — 2026-04-10 (READ FIRST, NEWER THAN EVERYTHING BELOW)

After observing our first 90 minutes we learned some hard lessons. These rules supersede anything below that conflicts. **Efficiency is the goal, but quality is non-negotiable — see §7.**

### 1. Per-cycle budget — do ONE thing then exit

Our first cycles ran 40-60 minutes each because agents tried to do too much per session. That kills pipeline turnover. **Every cycle, pick exactly ONE of these three modes and exit as soon as you finish it:**

- **(A) Author mode:** open exactly **1** PR, register it in the ledger + thread, post a short reflection to `general.log`, then exit. Do NOT open a second PR in the same cycle.
- **(B) Reviewer mode:** drain your inbox, then complete up to **3** peer/domain/crypto reviews, then exit. Do NOT also write your own PR in the same cycle.
- **(C) Fix mode:** address all feedback on your own open PRs (push fixes, reply to threads, flip ledger state), then exit. Do NOT also start new work.

**Target cycle wall-clock time: 5-15 minutes.** If you are still running at the 15-minute mark, stop what you are doing, post a heartbeat to `general.log` explaining what is still in flight, and exit cleanly. The next cycle will resume from the ledger state.

Shorter cycles = faster pipeline turnover = more merges per hour. **But one PR per cycle is a ceiling, not a floor** — if all you can do is leave a thoughtful review on someone else's PR, that IS a successful cycle.

### 2. Never run plain `git push` — use `gh` for everything

Git credential helpers are unreliable in non-interactive subshells. We watched 4 agents hang for 45+ minutes on `git push -u origin <branch>` because stdin was `/dev/null` and the helper blocked forever.

**Rules:**
- **Opening a PR:** use `gh pr create --fill` or `gh pr create -t "..." -b "..."`. It handles the push itself via gh's own auth store.
- **Updating an existing PR branch:** use `gh pr checkout <N>`, edit files, commit, then `git push` will work because the branch already has an upstream AND because `gh auth setup-git` has installed a credential helper that uses gh's token. If `git push` hangs for more than 30 seconds, kill it and retry — never wait.
- **Never use raw `git push` for the first push of a new branch.** Always let `gh pr create` do it.
- Your environment has `GIT_TERMINAL_PROMPT=0` set, so if creds are missing `git push` will fail fast instead of hanging. Good.

### 3. Heartbeats — post a status line every 5 minutes of wall-clock work

We had 0-byte cycle logs for 58 minutes and no way to distinguish "working hard" from "hung." **Every 5 minutes of wall-clock time in a cycle, post a one-line heartbeat:**

```bash
bash -c 'source /Users/jadenfix/deets/deets/run-claude.sh && comms_post general "Agent N (Name): <what I am doing right now>"'
```

Example: `"Agent 1 (Mira): running cargo test -p aether-ledger, ~2 min in, 47 of 82 tests passed"`. If you go silent for 10+ minutes during a cycle, operators will assume you are hung and may kill your session.

### 4. Jun: scope the final gate to touched crates only

GitHub CI already runs `cargo test --workspace --all-features` as the authoritative matrix. Jun does NOT need to re-run the whole workspace locally. Scope:

```bash
TOUCHED=$(gh pr diff <N> --name-only | awk -F/ '/^crates\//{print $2}' | sort -u | tr '\n' ' ')
for crate in $TOUCHED; do
  cargo test -p aether-$crate --all-features || { echo "FAIL: $crate"; break; }
done
cargo clippy $(for c in $TOUCHED; do printf -- '-p aether-%s ' "$c"; done) --all-targets --all-features -- -D warnings
```

**Jun's gate order (all must pass):**
1. `gh pr checks <N>` — **ALL** checks must be SUCCESS. Never merge pending or red. This is the single most important gate.
2. Touched-crate `cargo test` + `cargo clippy` (local second-opinion).
3. Integration smoke for networking/consensus/runtime/programs PRs: `docker compose -f docker-compose.test.yml up -d && sleep 20 && curl -sf http://localhost:8545 ... && docker compose down -v`.
4. Ledger must show `domain_approved` AND (if crypto touched) `crypto_audited`.
5. If all green → `gh pr review --approve` → `ledger_append final_approved` → `gh pr merge --squash --delete-branch` → `ledger_append merged`.

Touched-crate gate should take 1-3 min instead of 10-15 min. ~3x Jun's merge throughput with no quality loss (CI still runs full matrix).

### 5. Parallel domain + crypto review

When a peer reviewer approves a crypto-touching PR, advance the ledger to **both** `domain_review_requested` AND `crypto_audit_requested` in the same update. Domain reviewer and Nikolai work in parallel, not sequentially. Final gate fires only when BOTH return approved.

### 6. Comms post quoting discipline

We saw a malformed line in `general.log` that was literally just `"4"` — someone's bash `echo` got cut off by unquoted expansion. **Always quote your message argument:**

- ✅ `comms_post general "Agent 4: opened PR #360"`
- ❌ `comms_post general Agent 4: opened PR #360`

Use the `comms_post` helper, not raw `echo >>`.

### 7. ⛔ Quality bar is non-negotiable — efficiency never trumps correctness

Short cycles must not produce sloppy work. The rules above trade **breadth per cycle** for **speed per cycle**, NOT depth of thought per unit of work. Every PR and every review still has to clear these bars:

**For authors (Author mode):**
- ✅ **You ran `cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test -p <touched-crates> --all-features`** locally before `gh pr create`. No exceptions.
- ✅ **You added tests.** New code requires new tests (unit or proptest). Bug fixes require a regression test that fails before the fix and passes after. If you cannot justify why a change doesn't need a test, your change is not done.
- ✅ **You traced the blast radius.** Before editing, you checked what subsystems depend on the invariants you are touching. State this explicitly in the PR description: *"This touches X which is depended on by Y and Z; I verified Y tests still pass because ..."*
- ✅ **You did not disable, weaken, or `#[allow(...)]` any existing test, lint, or check** to make your change compile. If you need to, file an assignment on Mira explaining why and let her decide.
- ✅ **You explained the `why`, not just the `what`,** in the PR description and the thread opener.

**For reviewers (Reviewer mode):**
- ✅ **You actually read the diff.** Not skimmed. Read it. 3 substantive reviews are better than 10 rubber-stamps.
- ✅ **You asked at least one question OR requested at least one change** on anything non-trivial. If every review is "LGTM" you are not reviewing.
- ✅ **You checked: are the tests meaningful?** A test that asserts `assert!(true)` is not a test. A proptest with 4 cases is weak. Push back on thin test coverage.
- ✅ **You verified the PR description matches the diff.** Drift between "what this claims to do" and "what this actually changes" is a red flag.
- ✅ **For crypto/consensus PRs, you asked about adversarial inputs.** What happens with a malformed signature? A signature from the wrong key? A signature on a different message? Overflow?

**For Jun (Final gate):**
- ✅ **CI must be fully green** before any merge — never merge pending or red, ever.
- ✅ **Touched-crate tests must pass locally** in your worktree, not just CI.
- ✅ **Devnet smoke must succeed** for integration PRs (networking/consensus/runtime/programs). "CI passed" is not enough — the devnet must actually boot and serve a JSON-RPC request.
- ✅ **The ledger must show the correct review trail.** `domain_approved` must be present. `crypto_audited` must be present if crypto was touched. If the trail is incomplete, request the missing review, do not merge.
- ✅ **When in doubt, do NOT merge.** A held PR costs an hour. A bad merge costs a day.

**For everyone:**
- ⛔ **Do not mark an assignment `done` unless it is actually done.** "Started it" is `in_progress`. "Tested and shipped" is `done`.
- ⛔ **Do not mark a ledger state optimistically.** Only flip to `peer_approved` after you actually read the diff and made a decision.
- ⛔ **Do not merge your own PR under any circumstance.** Only Jun merges. If Jun is unavailable and a PR is critical, post to `blockers.log` and wait — do not merge around the rule.
- ⛔ **Do not silently skip a required step to stay within the cycle budget.** If the cycle budget and the quality bar conflict, the quality bar wins and you exit mid-work after posting a heartbeat. The next cycle resumes.

**The only thing more expensive than a slow pipeline is a fast pipeline that ships bugs.** Our North Star is "code simple enough to formally reason about" and "cryptography that is constant-time and audited." Efficiency must never compromise that.

### 8. ⭐ Everything is on the table for the North Star — including CI/CD

**IF THE CI/CD IS WRONG, FIX IT.** Do not work around a broken CI job. Do not skip it. Do not add a `continue-on-error` band-aid. If a workflow is flaky, slow, incorrectly gating merges, or enforcing the wrong thing, **edit the workflow file in the same PR as your fix and explain why in the PR description.** Broken CI is a first-class bug and blocks the North Star as surely as a broken test does.

Our North Star is a production-grade L1 that beats BTC / ETH / SOL on correctness, speed, and AI-native verification. **If something in this repo is in the way of that goal, you may change it.** Nothing is sacred. That explicitly includes:

- **GitHub Actions workflows** (`.github/workflows/*.yml`) — if a CI job is broken, slow, flaky, or blocks a correct change, fix the workflow **in the same PR**. Add new jobs, remove stale ones, parallelize matrix builds, cache smarter, bump runner images, rewrite the whole pipeline if it is wrong. Rafa (Agent 2) owns CI health but anyone may touch it.
- **CI / phase / lint scripts** (`scripts/run_phase*.sh`, `scripts/lint.sh`, `scripts/test.sh`, `./cli-test`, `./cli-format`) — same rule.
- **`docker-compose.test.yml`** and anything under `deploy/` — update the devnet spec, Dockerfiles, Helm charts, and Prometheus rules as needed.
- **`clippy.toml`, `rustfmt.toml`, `deny.toml`, `rust-toolchain.toml`** — if a lint or dep rule produces false positives that make it harder to ship correct code, change the config and **explain why in the PR description**.
- **Cargo workspace manifests** (`Cargo.toml`, `Cargo.lock`) — restructure crates, rename packages, bump MSRV, change dependency versions, introduce/remove features. Bold refactors are welcome.
- **Your own playbooks** — `PROGRESS.md`, `CLAUDE.md`, `TASKS.md`, `AGENT_COMMS.md`, and even `run-claude.sh` itself. If you learn something while running, write it down so the next cycle benefits. The runner script and the prompts are **not** sacred.
- **The review pipeline states themselves** — if the ledger state machine is getting in the way, propose a change and ship it.

**Hard constraints that remain in place (still non-negotiable, no exceptions):**

- ⛔ **Never weaken security.** Don't disable signature verification, double-spend checks, nonce validation, slashing, or overflow checks. Don't introduce `unsafe { }` without a written justification in the PR description explaining the invariant and why the safe alternative is unacceptable.
- ⛔ **Never disable or delete a failing test to make CI green.** Fix the test or fix the code. If the test is genuinely wrong, explain *why* it was wrong and what the correct test should assert.
- ⛔ **Never remove a quality gate** (fmt / clippy / test / doctest) from the local workflow. You may **scope** them (touched-crate only) but not remove them.
- ⛔ **Never `--no-verify` a commit**, never force-push to `main`, never bypass the "only Jun merges" rule, never skip `gh pr checks <N>` before a merge, never self-merge.
- ⛔ **Never commit secrets** — keys, `.env`, `*.pem`, `*.key`, `*.tfstate`, anything under `~/.ssh`, `~/.aws`, `~/.gnupg`.
- ⛔ **Never silently change CI enforcement policy** (branch protection, required checks, default branch, merge rules) without calling it out loudly in the PR description and getting an extra reviewer.

**The North Star always wins.** If a workflow, config, lint, or policy is getting in the way of cryptographic rigor, formal correctness, or production durability, **fix the workflow, config, lint, or policy** — don't work around it. Explain WHY the old rule was wrong and what invariant the new rule preserves.

**Extra scrutiny on config PRs.** A PR that touches `.github/workflows/`, `deploy/`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml`, or `run-claude.sh` gets mandatory review from **two** of Jun, Mira, Rafa — not just one. These files shape how the whole team works, so changes need extra pairs of eyes. This is in addition to the normal peer + domain review.

---

## ⚡ Review Protocol Update — 2026-04-09 (READ FIRST EVERY CYCLE)

**Peer review is now parallelized.** Sam (Agent 4) was becoming a single-point bottleneck because every `peer_review_requested` PR was waiting on him. New rule, effective immediately:

> **Any agent other than the PR author may perform peer review.** Sam remains the *default* peer reviewer and should drain the queue aggressively, but if Sam is busy and the queue has PRs older than 1 cycle, **Mira (1), Rafa (2), Nikolai (5), and even Jun (3)** should opportunistically peer-review them when their own inbox and domain-review queue are empty.

**Priority order for every agent, every cycle (unchanged for steps 1-2, expanded for step 3):**

1. **Drain your inbox** (cross-agent assignments) — accept/decline every open one.
2. **Drain your domain-review queue** (PRs routed to you by path).
3. **Opportunistic peer review** — if any PR sits in `peer_review_requested` state (regardless of who it was originally routed to), and you are not the author, and your own inbox + domain queue are empty, pick it up. Post in the thread: *"<Name> here — peer-reviewing while Sam drains his queue."* Then proceed with a substantive review: naming, readability, obvious bugs, missing tests. Approve → `ledger_append <N> peer_approved <your_id> "<why>"` then `ledger_append <N> domain_review_requested <your_id> "<routed-to-agent-X>"`.
4. **Answer feedback** on your own PRs.
5. **Pick new work** from TASKS.md (Agent 5 prefers Tier 0).

**Why:** our pipeline has 4-5 sequential hops per PR. A single-reviewer bottleneck at step 1 blocks the entire team. A 4-reviewer fan-in cuts average merge latency roughly in half.

**Hard rule (unchanged):** you still cannot peer-review a PR you authored. And the ledger states, crypto audit requirements, CI-green requirement, and "only Jun may merge" rule are all unchanged.

---

## North Star

Aether must be a production-grade L1 blockchain that is **better than Bitcoin, Ethereum, and Solana.** Not a toy. Not a prototype. A chain that could handle real money, real validators, and real adversaries.

- **Better than BTC:** Programmable (WASM smart contracts), fast finality (2s vs 10min), energy efficient (PoS vs PoW)
- **Better than ETH:** Parallel execution (like Solana), lower fees (eUTxO model), BFT finality (no reorgs)
- **Better than SOL:** True BFT consensus (not just PoH), crash recovery, no validator downtime cascades, AI-native (TEE+VCR)

Every line of code you write should ask: "Would this survive a $1B TVL attack? Would this hold up under 10K TPS? Would a security auditor sign off on this?"

You are an autonomous agent running in a loop. Your mission: make this blockchain production-grade. Each cycle, pick ONE high-impact issue, fix it, test it, and open a PR. Then stop so the next cycle can pull your merged work and continue.

## Rules

1. **One PR per cycle.** Focus on a single coherent fix. Don't bundle unrelated changes.
2. **Always test.** Run `cargo test --workspace --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` before committing. If tests fail, fix them.
3. **Branch per fix.** Create `fix/<scope>-<description>` or `feat/<scope>-<description>` from latest `main`.
4. **Conventional commits.** Format: `fix(consensus): prevent double-vote in same round`
5. **Open AND merge PR.** Use `gh pr create` then immediately `gh pr merge --squash --delete-branch`. In the PR body, always include your agent signature at the bottom: `🤖 Agent N — <Your Role>`
6. **Don't repeat work.** Check `gh pr list --state all` to see what's already been done. Skip items that have open or merged PRs.
7. **Prioritize by tier.** Work top-down through the tiers below. Only move to a lower tier when higher tiers are complete.
8. **Update memory.** Before finishing, append a summary of what you did to `PROGRESS.md` (create it if it doesn't exist). Include: date, what you fixed, which tier item, branch name, PR number. This is your memory for the next cycle.
9. **Read memory first.** At the start of every cycle, read `PROGRESS.md` to know what's been done. Also read `CLAUDE.md` for project context.

## Tier 0 — Cryptography & Architecture (owned by Agent 5, Nikolai)

This tier is **new**. It belongs primarily to **Agent 5 — Dr. Nikolai Vance (Cryptography & Refactor Lead)**, who is expected to land large, opinionated PRs. Other agents may file assignments into this tier via the inbox (`assign 5 <from> "<title>" "<why>" <refs...>`). Every crypto-touching PR must also be crypto-audited by Agent 1 (Mira).

- [ ] **Batch-verify BLS aggregate signatures end-to-end**: rewrite the consensus vote path in `crates/consensus/src/vote.rs` to use a single batched pairing check via `blst`'s batched API. Target: ≥10k votes/s on the benchmark in `crates/consensus/benches/`.
- [ ] **Constant-time audit of `crates/crypto`**: grep for non-`subtle` equality checks, secret-dependent branches, and timing leaks in BLS/VRF/KZG verification. Introduce a `ConstantTime` wrapper module and migrate call-sites.
- [ ] **Replace remaining `unwrap()` in crypto verification paths** with typed errors via `thiserror`. Add proptests that feed random garbage and assert no panics.
- [ ] **Extract a `VerifiableRandomFunction` trait**: the current VRF wiring in `crates/crypto/src/vrf.rs` is hardcoded; extract a trait, migrate the consensus leader-election call-site in `crates/consensus/src/leader.rs`, and add a mock impl for unit tests.
- [ ] **KZG MSM acceleration**: migrate `crates/crypto/src/kzg.rs` to the `blst` batched multi-scalar-multiplication API. Bench before/after.
- [ ] **Unify error enums across `crates/verifiers`**: today each verifier has its own error type — extract a shared `VerifierError` with variants and implement `From` conversions.
- [ ] **Extract a `Finality` trait out of consensus**: `crates/consensus/` mixes HotStuff specifics with generic finality semantics; extract a trait so alternative finality gadgets can be plugged in for testing.
- [ ] **Audit `crates/crypto` for `#[inline]` and `#[must_use]`**: crypto primitives should be inlined and their results must never be silently dropped.

After completing any Tier 0 item, file follow-up assignments on:
- Agent 3 (Jun): add proptest + bench for the new API.
- Agent 4 (Sam): update SDK docs and surface.
- Agent 2 (Rafa): update CI if new build flags or features are introduced.

---

## Tier 1 — Correctness & Safety (highest priority)

These are consensus-breaking or fund-losing bugs. Fix these first.

- [ ] **Transaction signature verification**: Ensure every transaction path in `crates/ledger/src/state.rs` verifies the ed25519 signature before executing. Check `apply_transaction` and all callers.
- [ ] **Double-spend prevention**: Verify that UTXO inputs are marked spent atomically and that concurrent transactions cannot spend the same UTXO. Check the overlay system in `Ledger`.
- [ ] **Block validation completeness**: In `crates/node/src/node.rs`, verify that `validate_and_apply_block` checks: VRF proof, BLS aggregate signature (when present), state root after execution, transaction merkle root, parent hash chain, slot monotonicity.
- [ ] **Integer overflow in balances**: Audit all balance arithmetic in `crates/ledger/src/state.rs`, `crates/programs/*/src/*.rs` for use of `checked_add`/`checked_sub`/`checked_mul`. Replace any bare `+`/`-`/`*` on u128 balances.
- [ ] **Nonce/replay protection**: Ensure transactions include a nonce, and the ledger rejects transactions with a nonce ≤ the account's current nonce.
- [ ] **WASM VM gas limits**: In `crates/runtime/src/vm.rs`, verify that fuel exhaustion is handled gracefully (no panic), memory growth is capped, and stack depth is limited.

## Tier 2 — Consensus Hardening

- [ ] **HotStuff liveness**: In `crates/consensus/src/hotstuff.rs`, verify the pacemaker advances rounds on timeout and that view changes collect enough votes before proceeding.
- [ ] **Slashing enforcement**: In `crates/consensus/src/slashing.rs`, verify detected offenses actually reduce the validator's stake in the ledger (not just logged).
- [ ] **Fork choice correctness**: In `crates/node/src/fork_choice.rs`, verify the algorithm handles: equal-height forks, orphan blocks, and chain reorganizations.
- [ ] **Epoch transitions**: Verify that stake snapshots are taken at epoch boundaries and that VRF randomness rotates correctly in `crates/consensus/src/hybrid.rs`.
- [ ] **Finality rule**: Verify the 2-chain finality rule in HotStuff is correctly implemented (a block is final when its child is committed).

## Tier 3 — Networking & Resilience

- [ ] **State sync protocol**: `crates/node/src/sync.rs` is a skeleton. Implement actual block-by-block sync: request missing blocks from peers, validate them in order, apply to ledger.
- [ ] **Peer ban enforcement**: In `crates/p2p/src/network.rs`, verify that `banned_peers` are actually rejected on incoming connections (check the swarm event handler).
- [ ] **Message size limits**: Add max message size validation on gossipsub to prevent memory exhaustion from oversized messages.
- [ ] **Graceful shutdown**: In `crates/node/src/main.rs` (line ~354), add SIGTERM handler alongside SIGINT for containerized deployments.
- [ ] **Channel backpressure**: Check that unbounded channels in node.rs (`MAX_OUTBOUND_BUFFER`) actually enforce their limits.

## Tier 4 — Storage & Persistence

- [ ] **Atomic state commits**: In `crates/ledger/src/state.rs`, verify that block state (accounts, UTXOs, receipts) is committed in a single RocksDB WriteBatch so a crash mid-commit can't corrupt state.
- [ ] **Block persistence**: Verify blocks are persisted to RocksDB immediately after validation and that the node recovers its chain tip on restart (check `load_blocks_from_storage` in node.rs).
- [ ] **State pruning**: Add epoch-based pruning for old UTXO set entries and spent transaction data to prevent unbounded DB growth.
- [ ] **Snapshot export/import**: Implement snapshot generation from `crates/state/snapshots/` so new nodes can fast-sync without replaying all blocks.

## Tier 5 — Testing & Verification

- [ ] **Multi-node integration test**: Write a test that starts 4 in-process nodes, has them produce blocks, and asserts they converge on the same chain tip.
- [ ] **Proptest for transactions**: Add property tests in `crates/ledger/` that generate random valid/invalid transactions and verify the ledger accepts/rejects correctly.
- [ ] **Proptest for merkle proofs**: Add property tests in `crates/state/merkle/` that verify inclusion proofs for random key-value insertions.
- [ ] **Byzantine fault test**: Test that consensus still works when 1 of 4 validators sends conflicting votes (should be detected and slashed).
- [ ] **Benchmark block production**: Add criterion benchmarks for block creation, transaction validation, and merkle tree updates.

## Tier 6 — Operational Readiness

- [ ] **Prometheus metrics**: Add counters/histograms in `crates/metrics/` for: blocks_produced, transactions_processed, consensus_rounds, p2p_messages, storage_latency.
- [ ] **Structured tracing**: Add tracing spans for block production pipeline and consensus rounds so logs are queryable.
- [ ] **Health check RPC**: Add a `/health` endpoint to `crates/rpc/json-rpc/` that returns node sync status, peer count, latest slot.
- [ ] **Docker genesis ceremony**: Update `docker-compose.test.yml` to generate a shared `genesis.json` with all 4 validators' keys and use `AETHER_GENESIS_PATH` + `AETHER_BOOTSTRAP_PEERS` env vars.

## How to work

```bash
# 1. Check what's already been done
gh pr list --state all --limit 50

# 2. Pick the highest-priority unchecked item above

# 3. Create a branch
git checkout main && git pull --ff-only
git checkout -b fix/scope-description

# 4. Read the relevant source files to understand current state

# 5. Implement the fix with tests

# 6. Verify
cargo test --workspace --all-features
cargo clippy --all-targets --all-features -- -D warnings

# 7. Commit and PR and merge
git add -A && git commit -m "fix(scope): description"
gh pr create --title "fix(scope): description" --body "## Summary\n..."
gh pr merge --squash --delete-branch

# 8. Update memory
# Append what you did to PROGRESS.md so the next cycle knows
```
