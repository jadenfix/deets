# CLAUDE.md — Aether Blockchain Project Context

## Project Identity

Aether is a Rust-first L1 blockchain with AI verification (TEE + VCR), a dual-token
economy (SWR staking + AIC compute credits), and a parallel execution runtime. This
is a monorepo with ~47 Rust crates, TypeScript and Python SDKs, web applications
(explorer, wallet), and deployment infrastructure.

## Workspace Layout

```
crates/                 Rust workspace (~47 crates)
  node/                 Validator binary (entry point)
  consensus/            VRF-PoS + HotStuff + BLS vote aggregation
  ledger/               eUTxO++ state transitions
  runtime/              WASM VM (wasmtime), parallel scheduler
  mempool/              Fee-priority tx pool, QoS
  p2p/                  libp2p networking
  networking/           quic-transport (Quinn), gossipsub
  da/                   turbine, erasure-coding (RS), shreds
  crypto/               primitives, vrf, bls, kes, kzg
  state/                merkle (sparse), storage (RocksDB), snapshots
  programs/             staking, governance, amm, job-escrow, reputation,
                        aic-token, account-abstraction
  verifiers/            tee, kzg-verifier, vcr-validator
  rpc/                  json-rpc, grpc-firehose
  types/                Shared types (Block, Transaction, Address, etc.)
  codecs/               Serialization (borsh, bincode)
  metrics/              Prometheus instrumentation
  light-client/         SPV verification
  mev/                  MEV mitigation
  rollup/               L2 rollup support
  sdk/                  rust, contract
  tools/                cli (aetherctl), keytool, faucet, scorecard,
                        indexer, loadgen
ai-mesh/               Off-chain AI workers: runtime, router, coordinator, worker
sdks/                  typescript/, python/
apps/                  explorer/, wallet/
packages/              ui/ (shared components)
deploy/                docker/, helm/, k8s/, terraform/, prometheus/, grafana/
scripts/               lint.sh, test.sh, devnet.sh, docker-test.sh,
                       run_phase{1..7}_acceptance.sh, chaos/
fuzz/                  libfuzzer targets (transaction, block, merkle, vrf, wasm)
config/                genesis.toml
docs/                  architecture.md, ops/, security/, grants/, phase7/
```

## Build & Test Commands

### Primary (CI-aligned)

```bash
cargo build --workspace                    # Debug build
cargo build --release                      # Release build
cargo test --all-features --workspace      # All Rust tests (418+ tests)
cargo test --doc --all-features --workspace # Doc tests
./cli-test                                 # Full test suite (Rust + JS/TS)
./cli-test --rust-only                     # Rust tests only
./cli-format                               # Lint: fmt + clippy + check
```

### Makefile Shortcuts

```bash
make build     # cargo build --release
make test      # ./cli-test --rust-only
make lint      # ./cli-format
make proptest  # cargo test --all --features proptest -- --ignored
make docs      # cargo doc --no-deps --all
make clean     # cargo clean + docker compose down
make devnet    # Build + docker compose up (4 validators)
make bench-parallel  # cargo bench --package aether-runtime --bench scheduler
```

### Phase Acceptance Suites

```bash
./scripts/run_phase1_acceptance.sh  # Ledger, consensus, mempool
./scripts/run_phase2_acceptance.sh  # Staking, governance, AMM, AIC, escrow
./scripts/run_phase3_acceptance.sh  # AI mesh, TEE, VCR, KZG, reputation
./scripts/run_phase4_acceptance.sh  # Networking, DA, performance
./scripts/run_phase5_acceptance.sh  # SRE, observability, metrics
./scripts/run_phase6_acceptance.sh  # Security, crypto audit
./scripts/run_phase7_acceptance.sh  # CLI, SDKs, tooling
```

### TypeScript / Python / Docker

```bash
npm ci && npm run test:ts                    # All TS tests
cd sdks/python && pip install -e .[dev] && PYTHONPATH=src python3 -m pytest
docker compose -f docker-compose.test.yml up -d   # 4-node test network
```

## Rust Conventions

- **Edition**: 2021, MSRV 1.75+
- **Formatter**: rustfmt.toml — max_width=100, Unix newlines, reorder imports
- **Linter**: clippy.toml — cognitive-complexity-threshold=30
- **Deps**: deny.toml blocks openssl/openssl-sys (use rustls); crates.io only
- **Error handling**: anyhow for binaries, thiserror for libraries
- **Async**: tokio full features, async-trait
- **Serialization**: serde + bincode for wire, borsh for state, serde_json for RPC
- **Testing**: proptest for property tests, criterion for benchmarks
- **Profiles**: test builds use opt-level=2; crypto crates get opt-level=3 even in test

## Commit Style

Conventional commits: `feat(scope)`, `fix(scope)`, `docs(scope)`, `test(scope)`, `refactor(scope)`

Scopes: consensus, runtime, ledger, rpc, programs, ai-mesh, ops, sdk, ui, docs, crypto, da, networking, mempool, node, tools

## Architecture

### Consensus Path
Transaction -> Mempool (gossipsub) -> VRF leader election -> Block proposal ->
Turbine sharding (RS erasure) -> Parallel WASM execution (R/W set analysis) ->
BLS vote aggregation -> HotStuff finality (>=2/3 stake) -> State commitment
(Sparse Merkle) -> Receipts -> Indexer

### AI Job Flow
Job posted (AIC escrow) -> Router selects provider -> Provider accepts (stakes bond) ->
TEE execution -> VCR generation (quote + KZG commits) -> On-chain submit ->
Challenge window -> Watchtower verification -> Settlement

### Key Devnet Ports
- 8545-8548: JSON-RPC (validators 1-4)
- 9000-9003: P2P gossip (validators 1-4)
- 9090: Prometheus metrics
- 3000: Grafana dashboard

## Current Status

Phases 1-6 complete (consensus, programs, AI mesh, networking/DA, SRE, security).
Phase 7 (developer platform) scaffolded: SDKs exist with local stubs, explorer
and wallet have mock data.

## Trust Boundaries (Unattended Runs)

- NEVER read from: keys/, .env, .env.local, *.pem, *.key, *.tfstate, ~/.ssh, ~/.aws, ~/.gnupg
- NEVER force-push or publish (cargo publish, npm publish)
- NEVER run kubectl/helm/terraform apply
- NEVER curl/wget to domains outside: github.com, crates.io, npmjs.org, pypi.org, docs.rs
- All writes confined to repo directory + /tmp/claude-build
- Docker compose allowed for local test networks only
- Model: always claude-opus-4-6 with 1M context

## Autonomous Loop Protocol (5-Agent Crew)

- You run as one of **5 agents** launched by `run-claude.sh`: Mira (1, consensus), Rafa (2, full-stack/ops), Jun (3, quality & final gate), Sam (4, peer reviewer/generalist), Nikolai (5, crypto & refactor lead).
- **The protocol spec lives in [AGENT_COMMS.md](AGENT_COMMS.md)** — read it every cycle. It defines the PR review ledger, cross-agent task delegation (inbox), per-PR dialogue threads, expertise routing, and dialogue norms.
- **Memory**: Read `PROGRESS.md` at the start of every cycle. Append to it before finishing. This is your cross-cycle memory.
- **No self-merge.** Only **Agent 3 (Jun)** may call `gh pr merge`, and only when the PR ledger state is `final_approved`. If your PR is blocked > 3 cycles, post to `blockers.log` and move on.
- **Multi-step review:** `author_ready → peer_review_requested → peer_approved → domain_review_requested → domain_approved → [crypto_audit_requested → crypto_audited] → final_approval_requested → final_approved → merged`. Any reviewer may set `changes_requested`.
- **Delegate freely.** Drain your **Inbox** (`assignments_for <id>`) before picking new work from `TASKS.md`. When you hit something outside your lane, file an assignment on the right agent via `assign <to> <from> "<title>" "<why>" <refs...>`.
- **Task list**: `TASKS.md` has prioritized tiers (Tier 0 = Cryptography & Architecture, owned by Agent 5). Work top-down in your ownership area.
- **Testing is mandatory.** Run `cargo fmt/clippy/test` before every PR. If your change touches networking/consensus/runtime/programs, ALSO bring up the devnet via `docker compose -f docker-compose.test.yml up -d` and hit the RPC with curl. You have full Docker access; use it.
- **One coherent PR per cycle** — but feel free to make it big if the change is cross-cutting. Breaking internal APIs is fine as long as you update all callers in the same PR.
