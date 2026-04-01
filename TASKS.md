# Aether Blockchain — Autonomous Production Hardening

You are an autonomous agent running in a loop. Your mission: make this blockchain production-grade (ETH/BTC level). Each cycle, pick ONE high-impact issue, fix it, test it, and open a PR. Then stop so the next cycle can pull your merged work and continue.

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
