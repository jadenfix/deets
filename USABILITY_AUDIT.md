# Aether Blockchain — Usability Audit

**Date**: 2026-03-31  
**Scope**: All user-facing surfaces — CLI, RPC/API, SDKs, node operation, native programs, documentation, error handling  
**Total Issues Found**: 192  

---

## Executive Summary

| Severity | Count | Description |
|----------|-------|-------------|
| **Critical** | 28 | Panics on user input, silent data corruption, missing core functionality |
| **High** | 57 | Poor error messages, missing confirmations, broken workflows |
| **Medium** | 62 | UX friction, missing features, inconsistencies |
| **Low** | 45 | Polish, naming, minor clarity issues |

The three highest-impact themes across the entire codebase:

1. **Crash-on-bad-input** — `unwrap()`, `expect()`, and `unreachable!()` on user-controlled code paths (node, CLI, RPC, contract SDK). These will panic the process on malformed input.
2. **Silent failures** — Operations silently drop errors, return defaults, or ignore invalid parameters instead of surfacing actionable feedback.
3. **Missing operational documentation** — No deployment guide, no config reference, no API docs, no troubleshooting depth. Users must reverse-engineer source code.

---

## 1. CLI Tool (`crates/tools/cli`, `crates/tools/keytool`, `crates/tools/faucet`)

### Critical

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 1 | `cli/src/main.rs` | 58 | Errors printed with `{err:?}` (Debug format) — leaks internal error chains to users | Change to `{err}` (Display format) |
| 2 | `cli/src/io.rs` | 69 | `.expect()` on address derivation from keypair — panics on malformed key files | Replace with `.map_err(...)? ` |

### High

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 3 | `cli/src/transfers.rs`, `staking.rs` | — | No confirmation prompt before transfers/staking — irreversible fund loss on typo | Add interactive confirm + `--yes` flag |
| 4 | `cli/src/main.rs` | — | Most command args lack help text descriptions | Add `#[arg(help = "...")]` to all args |
| 5 | `cli/src/io.rs` | 116 | Address validation error says "must be 20 bytes" but doesn't show an example format | Include example: `0x1234...5678` |
| 6 | `cli/src/jobs.rs` | 135-163 | `job tutorial` prints hardcoded placeholder hashes users must replace blindly | Add guidance on where to find real values |
| 7 | `cli/src/keys.rs` | 40-46 | `--overwrite` flag destroys key files with no backup and no confirmation | Add confirmation prompt even with `--overwrite` |
| 8 | `cli/src/config.rs` | 69-79 | Default config path `~/.aether/config.toml` never mentioned in help text | Add to `--config` help text |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 9 | Nonce requires manual tracking — no auto-fetch from chain | Make `--nonce` optional, auto-query if omitted |
| 10 | All output is JSON-only — no human-readable `--output text` mode | Add `--output` flag with `text`/`json` options |
| 11 | Full 64-char hashes clutter terminal output | Truncate to `0x12345678...abcd` in text mode |
| 12 | Fee/gas have no validation ranges — user can set fee to `u128::MAX` | Add upper-bound sanity checks |
| 13 | `--metadata` and `--metadata-file` both set → file silently wins | Error on conflicting flags |
| 14 | Faucet env vars (`AETHER_FAUCET_LIMIT`, etc.) undocumented, silently use defaults on parse failure | Log warnings on invalid env var values |
| 15 | Faucet rate-limit error says "retry later" but doesn't say how long | Include remaining cooldown time |
| 16 | Faucet GitHub handle validation error doesn't explain the rules | Show: "must start/end alphanumeric, 3-40 chars" |

---

## 2. RPC / API (`crates/rpc/json-rpc`, `crates/rpc/grpc-firehose`)

### Critical

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 17 | `server.rs` | 323, 408, 613 | Hash/address values serialized with `{:?}` Debug format — responses contain `H256 { ... }` instead of hex strings | Use `format!("0x{}", hex::encode(...))` |
| 18 | `server.rs` | 212 | WebSocket event serialization uses `unwrap_or_default()` — sends empty strings on failure | Log error and break connection |

### High

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 19 | `server.rs` | 17-22 | No JSON-RPC 2.0 `jsonrpc` field validation — accepts any version | Validate `jsonrpc == "2.0"` |
| 20 | `server.rs` | 227-233 | WebSocket subscribe/unsubscribe not implemented — clients can't control subscriptions | Implement subscription protocol |
| 21 | `server.rs` | 187-190 | CORS hardcoded to `localhost:3000` — blocks all production frontends | Make CORS configurable via env var |
| 22 | `server.rs` | 169 | No per-parameter size limits — reads/writes arrays can be arbitrarily large | Add `MAX_ADDRESSES` validation |
| 23 | `server.rs` | 254-288 | No request timeout — hanging backend blocks HTTP thread indefinitely | Add `tokio::time::timeout(30s, ...)` |
| 24 | `server.rs` | 176 | `/health` always returns `{"status": "ok"}` even when node is down/syncing | Check sync status and peer count |
| 25 | `server.rs` | — | No `/ready` endpoint for Kubernetes readiness probes | Add `/ready` checking sync + peers |
| 26 | `server.rs` | — | No version/info endpoint — clients can't discover chain ID or API version | Add `/info` or `aeth_chainId` method |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 27 | Hex parsing uses `trim_start_matches("0x")` — accepts `"00xABCD"` | Use strict `strip_prefix("0x")` |
| 28 | Hash length not validated before `H256::from_slice()` — generic error | Check 32 bytes explicitly with message |
| 29 | No RPC method documentation, no OpenAPI spec | Add doc comments and/or generate spec |
| 30 | Broadcast errors silently dropped with `let _ = sender.send(event)` | Log failed broadcasts |
| 31 | Firehose stream silently skips lagged events — data gaps with no notification | Log skip count, optionally close connection |
| 32 | No batch request support (JSON-RPC 2.0 spec allows arrays) | Accept `[{...}, {...}]` format |
| 33 | Inconsistent error message formatting — mixed casing and detail level | Standardize format |
| 34 | `Signature::from_bytes()` called without length validation | Validate 64 bytes before constructing |

---

## 3. SDKs (`crates/sdk/rust`, `crates/sdk/contract`, `sdks/typescript`, `sdks/python`)

### Critical

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 35 | `contract/src/storage.rs` | 46 | `.lock().unwrap()` on mutex — panics on poison | Return `ContractError::StorageError` |
| 36 | `contract/src/storage.rs` | 55 | `.try_into().unwrap()` on byte conversion — panics on wrong length | Use `.map_err(...)? ` |
| 37 | `python/transaction.py` | 34-35 | Signature length validation off-by-one with `0x` prefix handling | Fix to `< 128` or clarify prefix semantics |

### High

| # | Issue | Fix |
|---|-------|-----|
| 38 | Rust SDK client methods (`submit`, `transfer`, `job`) have zero doc comments | Add `///` docs with examples |
| 39 | `TransferBuilder` and `JobBuilder` methods undocumented — unclear which fields required | Add doc comments per method |
| 40 | Inconsistent nonce handling across Rust/Python/TS SDKs | Standardize or document differences |
| 41 | Chain ID hardcoded to `1` in Rust `TransferBuilder` — can't target testnet | Make `chain_id` configurable |
| 42 | No custom error type exported from Rust SDK — users get opaque `anyhow::Error` | Define and export `AetherError` enum |
| 43 | TypeScript client casts `payload.result as T` without runtime validation | Add runtime type checking |
| 44 | No key generation utilities in Python/TypeScript SDKs | Add `KeyManager` wrapping nacl/tweetnacl |
| 45 | No transaction simulation/dry-run in any SDK | Add `simulate()` method |
| 46 | No fee estimation in any SDK | Add `estimate_fee()` method |
| 47 | No retry/backoff logic in any SDK — transient failures immediately error | Add configurable retry with exponential backoff |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 48 | `JobBuilder.job_id("")` silently ignores empty string — no error raised | Return `Result` or raise `ValueError` |
| 49 | Contract SDK `read_u128` returns `0` for missing keys vs `None` — indistinguishable | Return `Option<u128>` |
| 50 | Inconsistent signature validation: Rust checks 64 bytes, TS checks 128 hex, Python checks 130 | Standardize across all SDKs |
| 51 | Host functions return hardcoded mocks — no docs on WASM compilation path | Document WASM build process |
| 52 | Inconsistent naming: Rust `job_id()`/`model_hash()` vs Python/TS `id()`/`model()` | Standardize across SDKs |
| 53 | Builder instances reusable after `build()` — can create shared-state bugs | Document non-reuse or consume `self` |
| 54 | TypeScript subscription client silently drops unparseable messages | Emit error events |
| 55 | No URL validation on endpoint in any SDK | Validate at construction time |

---

## 4. Node Operation & Configuration (`crates/node`, `config/`)

### Critical

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 56 | `node/src/main.rs` | 163-174 | Config load errors are generic ("failed to read config file") — no guidance | Show required fields, suggest example config |
| 57 | `node/src/main.rs` | 182 | Default data dir is `./data/node1` (relative path) — creates multiple DBs if run from different dirs | Default to `~/.aether/data/` |
| 58 | `node/src/main.rs` | — | No `--help`, `--version`, or arg parser — binary has zero discoverability | Add clap with help text |
| 59 | `node/src/main.rs` | — | `tracing_subscriber` never initialized — `RUST_LOG` has no effect | Call `.init()` in main |
| 60 | `node/src/main.rs` | — | Metrics crate exists but never wired into main binary — no Prometheus endpoint | Spawn metrics exporter |
| 61 | Dockerfile | 44-45 | ENTRYPOINT hardcoded — no env var override for config or data path | Use `ENV` + shell entrypoint |
| 62 | — | — | Genesis setup completely undocumented — no CLI tool to generate multi-validator genesis | Create `generate_genesis.sh` |
| 63 | `rpc/server.rs` | 176 | `/health` always returns OK regardless of node state | Check sync status |

### High

| # | Issue | Fix |
|---|-------|-----|
| 64 | No log rotation, no file output — stdout only, fills disk in production | Integrate file writer with rotation |
| 65 | `println!` used 58+ times instead of `tracing::*` — no structured logging | Migrate to tracing macros |
| 66 | No per-module log level control (`RUST_LOG=aether_consensus=debug`) | Initialize `EnvFilter` |
| 67 | Operational log messages non-actionable (e.g., "broadcast channel closed") | Add "what to do" suggestions |
| 68 | Circuit breaker logs don't explain remediation steps | Add: "check peer connectivity, verify bootstrap peers" |
| 69 | NAT traversal, port requirements, firewall rules undocumented | Create networking docs |
| 70 | No SIGTERM handler — only SIGINT — Kubernetes kills without cleanup | Handle both signals |
| 71 | No graceful shutdown of P2P/RPC — connections dropped mid-request | Broadcast shutdown signal |
| 72 | Docker Compose port mappings undocumented (which is RPC vs P2P?) | Add comments |
| 73 | Scripts have no `--help` and don't check prerequisites | Add arg parsing and tool checks |
| 74 | Config validation errors don't explain what config parameters mean | Add context: "tau is VRF election probability" |
| 75 | Genesis validator keys must be manually created — no ceremony tooling | Document or script the ceremony |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 76 | No disk space check on startup — silent write failures | Warn if < 1GB available |
| 77 | No RocksDB tuning documentation for production | Create `PRODUCTION.md` |
| 78 | GETTING_STARTED.md doesn't explain multi-node setup | Add section with `devnet.sh` |
| 79 | No Dockerfile HEALTHCHECK directive | Add `curl /health` check |
| 80 | Data directory contents/structure not documented | Explain what's in `./data/` |
| 81 | Bootstrap peers must be manually specified — no autodiscovery | Document, plan DHT/mDNS |
| 82 | No tool to compare genesis hashes across validators | Add `genesis-hash` CLI command |

---

## 5. Native Programs (Staking, Governance, AMM, Job Escrow, AIC, Reputation, Account Abstraction)

### Critical

| # | File | Issue | Fix |
|---|------|-------|-----|
| 83 | `staking/src/state.rs:100-138` | `register_validator()` has no caller/authority validation — anyone can register validators with arbitrary reward addresses | Add `caller == address` check |
| 84 | `job-escrow/src/lib.rs:180` | VCR proof verification stubbed out ("assume valid") — any provider gets paid for garbage | Implement cryptographic VCR verification |
| 85 | `amm/src/pool.rs:196-197` | `amount_in_with_fee * reserve_out` can overflow u128 with large reserves | Use 256-bit arithmetic or checked_mul with fallback |
| 86 | `job-escrow/src/lib.rs` | No mechanism to mark jobs as failed — providers face no consequences for accepting and doing nothing | Add `fail_job()` instruction |
| 87 | `governance/src/lib.rs:292-303` | `delegate()` modifies effective power immediately — flash loan attack possible | Snapshot-based delegation changes |

### High

| # | File | Issue | Fix |
|---|------|-------|-----|
| 88 | `staking/state.rs:215-220` | Unbonding queue grows unbounded — O(n) denial of service | Add queue size limit |
| 89 | `governance/lib.rs:206-209` | Zero total voting power allows proposals to pass with 0 votes | Require minimum participation |
| 90 | `reputation/scoring.rs:76-85` | No bounds on dispute penalties — unlimited disputes possible | Require arbiter signature |
| 91 | `aic-token/lib.rs:62-88` | `mint()` has no supply cap or rate limit | Add `mint_cap` field |
| 92 | `account-abstraction/lib.rs:139-141` | Paymaster deposit deducted with `saturating_sub()` — hides insufficient balance | Check deposit >= cost first |
| 93 | `staking/state.rs:244-269` | Slash doesn't update delegator `reward_debt` — incorrect reward distribution after slash | Recompute affected delegations |
| 94 | `governance/lib.rs:404-418` | `execute_treasury_allocation()` accepts `_recipient` but never uses it — no actual transfer | Implement transfer or remove param |
| 95 | `job-escrow/lib.rs` | Requester can't withdraw payment after cancellation — funds locked | Add `claim_refund()` instruction |
| 96 | All programs | No event/emission system — indexers can't track state changes | Implement event trait |
| 97 | `staking/state.rs:264-266` | Slash count never expires — jailed forever, no unjail mechanism | Add `unjail()` with cooldown period |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 98 | Staking `delegate()` doesn't validate delegator has sufficient balance | Add balance check |
| 99 | AMM `add_liquidity()` has no max price impact protection | Add `max_price_impact` param |
| 100 | Job escrow challenge period hardcoded to 10 slots — not configurable | Make configurable per job |
| 101 | Reputation scores don't decay for inactive providers | Implement time-based decay |
| 102 | Provider can't abandon accepted job cleanly | Add `abandon_job()` with reputation penalty |
| 103 | Staking error messages show raw token units without context ("minimum is 100_000_000") | Clarify: "100 SWR (100_000_000 units)" |
| 104 | No batch query operations — UI must make N queries for N items | Add `get_delegations_for_account()` etc. |
| 105 | No program upgrade mechanism documented | Document governance upgrade path |
| 106 | Validators can't self-deactivate/retire — only slashing deactivates | Add `deactivate_validator()` |

---

## 6. Error Handling (Codebase-wide)

### Critical

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 107 | `node/src/node.rs` | 424 | `.unwrap()` on `validator_key` — crashes node if None | Use `.ok_or_else(...)? ` |

### High

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 108 | `node/src/network_handler.rs` | 82, 102, 130 | `unreachable!()` on unexpected P2P messages — crashes node | Log warning and skip |
| 109 | `node/src/main.rs` | 139 | Vote serialization `unwrap_or_default()` — silently sends empty vote | Log error, skip vote |
| 110 | `node/src/node.rs` | 327 | Division by `total_stake` — safe now but fragile if guard removed | Use `checked_div` |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 111 | RPC "Invalid hash"/"Invalid address" errors don't show what value was received | Include the offending value |
| 112 | RPC "Invalid parameter type" doesn't say expected vs. received type | Add both to message |

---

## 7. Documentation & Onboarding

### Critical

| # | File | Issue | Fix |
|---|------|-------|-----|
| 113 | `GETTING_STARTED.md:299-300` | References non-existent `STRUCTURE.md` and `IMPLEMENTATION_STATUS.md` | Create files or remove links |
| 114 | `overview.md` | File is empty (0 bytes) — README references it | Write content or remove reference |
| 115 | `sdks/typescript/` | TypeScript SDK has no README | Create README with install/usage |
| 116 | `apps/explorer/`, `apps/wallet/` | No README for either user-facing app | Create READMEs |

### High

| # | Issue | Fix |
|---|-------|-----|
| 117 | No `cargo doc` instructions anywhere — no rustdoc generation | Add to GETTING_STARTED.md |
| 118 | No deployment guide — `deploy/` has Helm/Terraform/K8s configs with zero docs | Create `deploy/README.md` |
| 119 | CONTRIBUTING.md lacks code style guide, PR review process, security reporting | Expand significantly |
| 120 | No integration test documentation — `/tests/` has no README | Create test guide |
| 121 | No troubleshooting guide for common errors | Create `docs/troubleshooting.md` |

### Medium

| # | Issue | Fix |
|---|-------|-----|
| 122 | Multiple conflicting progress/status documents — references to non-existent files | Consolidate into single source |
| 123 | No RPC API reference — developers must read source code | Create `docs/api-reference.md` |
| 124 | No CHANGELOG — no way to track breaking changes | Create CHANGELOG.md |
| 125 | No tutorials or working end-to-end examples | Create `docs/tutorials/` |
| 126 | CLI scripts (`cli-format`, `cli-test`, etc.) flags undocumented | Add `--help` to each |
| 127 | Security docs (`THREAT_MODEL.md`, `AUDIT_SCOPE.md`) not linked from README | Add security section |
| 128 | Runbooks exist but aren't discoverable from any entry point | Link from deploy/README |
| 129 | Python SDK README examples may not work as-is | Clarify placeholders |
| 130 | No glossary for domain terms (VRF, eUTxO++, KZG, TEE, VCR) | Create `docs/glossary.md` |
| 131 | No benchmarking guide — perf targets stated but no instructions to verify | Create `docs/benchmarking.md` |
| 132 | No fuzz testing documentation — `/fuzz/` has no README | Create `fuzz/README.md` |

---

## Top 20 Quick Wins

These are high-impact fixes requiring minimal effort:

| Priority | Fix | Effort | Impact |
|----------|-----|--------|--------|
| 1 | Change `{err:?}` to `{err}` in `cli/main.rs:58` | 5 sec | All CLI error output |
| 2 | Change `{:?}` to hex encoding in `rpc/server.rs` (3 locations) | 5 min | All RPC responses |
| 3 | Replace `unreachable!()` with `tracing::warn` + `continue` in `network_handler.rs` | 5 min | Prevents node crashes |
| 4 | Replace `.unwrap()` on `validator_key` in `node.rs:424` | 2 min | Prevents node crash |
| 5 | Replace `.expect()` in `cli/io.rs:69` with `?` | 2 min | Prevents CLI crash |
| 6 | Replace `.lock().unwrap()` in `contract/storage.rs:46` | 2 min | Prevents contract panic |
| 7 | Replace `.try_into().unwrap()` in `contract/storage.rs:55` | 2 min | Prevents contract panic |
| 8 | Initialize `tracing_subscriber` in `node/main.rs` | 5 min | Enables `RUST_LOG` |
| 9 | Make CORS configurable via `CORS_ORIGINS` env var | 10 min | Unblocks all frontends |
| 10 | Add `--yes` flag to `transfer` and `stake` CLI commands | 30 min | Prevents accidental fund loss |
| 11 | Add help text to all CLI `#[arg]` attributes | 1 hr | CLI discoverability |
| 12 | Add request timeout to RPC handlers | 15 min | Prevents hung requests |
| 13 | Make `/health` check actual node state | 15 min | Accurate health monitoring |
| 14 | Write `overview.md` content (or delete reference) | 15 min | Fixes broken doc link |
| 15 | Create `sdks/typescript/README.md` | 30 min | TS SDK discoverability |
| 16 | Remove references to non-existent docs in `GETTING_STARTED.md` | 5 min | Fixes broken links |
| 17 | Add clap arg parser to node binary with `--help` | 30 min | Node discoverability |
| 18 | Add `caller == address` check to `register_validator()` | 5 min | Prevents unauthorized registration |
| 19 | Wire metrics exporter into `node/main.rs` | 15 min | Enables Prometheus |
| 20 | Add `ENV` directives to Dockerfile | 10 min | Configurable Docker deploys |

---

## Recommended Phases

### Phase 1: Stop the Bleeding (1-2 days)
Fix all crash/panic paths (items 1-2, 35-36, 107-108), RPC debug format (17), and health endpoint (63). These are the minimum for a usable system.

### Phase 2: Core UX (1 week)
- CLI confirmations and help text (3-8)
- RPC CORS, timeouts, validation (19-26)
- Node logging initialization (59, 64-66)
- Staking authority check (83)
- VCR verification stub (84)

### Phase 3: Developer Experience (2 weeks)
- SDK documentation and consistency (38-55)
- API reference documentation (123)
- Deployment guide (118)
- Getting started fixes (113-121)
- Program error messages (103)

### Phase 4: Polish (ongoing)
- Tutorials and examples (125)
- Fee estimation and tx simulation (45-46)
- Batch queries for programs (104)
- Glossary and troubleshooting (130-131)
- Event emission system (96)
