# Aether Blockchain - Implementation Roadmap

## Progress Overview

**Current Status**: Phases 1-6 implemented; Phase 7 scaffolded. Active hardening push in progress.

---

### Phase 0: Foundation -- COMPLETE
- [x] Repository structure and Cargo workspace (47+ crates)
- [x] Core type system (H256, Address, Signature, Transaction, Block, Receipt)
- [x] eUTxO++ transaction model with R/W conflict detection
- [x] CI: build, lint, test, Docker, multi-arch

### Phase 1: Core Ledger & Consensus -- COMPLETE
- [x] Ed25519 signature verification (ed25519-dalek, batch verify)
- [x] JSON-RPC server (8 methods, `aeth_*` namespace)
- [x] ECVRF leader election (structure complete; crypto placeholder being replaced)
- [x] BLS12-381 vote aggregation (blst-backed, 1.7k verifications/s)
- [x] HotStuff 2-chain BFT consensus
- [x] WASM runtime skeleton (gas metering; Wasmtime integration in progress)
- [x] Parallel scheduler (R/W conflict detection)
- [x] P2P networking (gossipsub module; libp2p wiring in progress)

### Phase 2: Economics & System Programs -- COMPLETE
- [x] Staking program (bond/unbond/delegate/slash)
- [x] Governance program (proposals, voting, quorum, timelock)
- [x] AMM DEX (constant product x*y=k, LP tokens)
- [x] AIC token (deflationary, mint/burn)
- [x] Job escrow (post/accept/submit VCR/challenge/settle)
- [x] Reputation program

### Phase 3: AI Mesh & Verifiable Compute -- PARTIAL
- [x] TEE attestation types (SEV-SNP, TDX, Nitro)
- [x] VCR validation structure (quorum consensus)
- [x] KZG commitment types (structure; pairing implementation in progress)
- [x] AI worker and coordinator (code exists but disabled in workspace)
- [ ] Real TEE cert-chain verification (x509)
- [ ] Real KZG pairing-based proofs
- [x] VCR validator wired to TEE + KZG verifiers (simulation-grade wiring)
- [ ] AI mesh workspace members re-enabled in CI

### Phase 4: Networking, DA & Performance -- COMPLETE
- [x] Turbine block propagation (tree topology, broadcast + repair)
- [x] Reed-Solomon erasure coding RS(10,2) -- 167 MB/s encode, 572 MB/s decode
- [x] Batch signature verification -- Ed25519: 105k sig/s, BLS: 1.7k/s
- [x] QUIC transport (Quinn + TLS 1.3)
- [x] Data availability tests (packet loss, Byzantine, stress)

### Phase 5: SRE & Observability -- PARTIAL
- [x] Prometheus metrics (60+ across consensus, DA, networking, runtime, AI)
- [x] Grafana dashboard with SLO tracking
- [x] Alert rules (10+)
- [x] Metrics HTTP exporter on port 9090
- [ ] Helm charts (validator, rpc, indexer, prom/grafana)
- [ ] Terraform provider modules (AWS/GCP/Azure)
- [ ] Runbooks (incident triage, rollback, key-loss)
- [ ] Chaos testing suite
- [ ] State-sync protocol

### Phase 6: Security & Formal Methods -- PARTIAL
- [x] STRIDE/LINDDUN threat model (23 threats)
- [x] TLA+ specification (HotStuff safety/liveness)
- [x] KES key rotation protocol
- [x] Remote signer architecture design
- [ ] Coq/Isabelle formal proofs
- [ ] Full external audit execution
- [ ] Chaos/fault injection testing

### Phase 7: Developer Platform & Ecosystem -- SCAFFOLDED
- [x] Rust SDK (job builder, tutorials)
- [x] TypeScript SDK (transactions, jobs, explorer hooks)
- [x] Python SDK (transactions, job tooling)
- [x] aetherctl CLI (transfer/staking/job flows + tests)
- [x] Explorer scaffold (mock data; live RPC in progress)
- [x] Wallet scaffold (demo keys; real signing in progress)
- [x] Faucet and scorecard automation
- [x] SDKs wired to real RPC
- [x] Explorer/wallet connected to live node (with mock fallback)
- [x] Indexer implementation
- [x] Load generator implementation
- [x] Keytool implementation
- [x] CONTRIBUTING.md

---

## Active Hardening Push

The `jaden/big-pushg` branch is closing all gaps across phases 1-7:
- Core wiring: Node + RPC + P2P integration
- Crypto hardening: Real VRF, KZG, verification pipelines
- Runtime: Wasmtime integration, host function binding
- AI mesh: Re-enable workspace members, wire coordinator
- Interfaces: SDK real RPC calls, explorer/wallet live data
- Tooling: Indexer, loadgen, keytool
- Ops: Helm, Terraform modules, runbooks, chaos tests
- Docs: Reconcile all status documents

---

## Acceptance Scripts

| Phase | Script |
|-------|--------|
| 1 | `./scripts/run_phase1_acceptance.sh` |
| 2 | `./scripts/run_phase2_acceptance.sh` |
| 3 | `./scripts/run_phase3_acceptance.sh` |
| 4 | `./scripts/run_phase4_acceptance.sh` |
| 5 | `./scripts/run_phase5_acceptance.sh` |
| 6 | `./scripts/run_phase6_acceptance.sh` |
| 7 | `./scripts/run_phase7_acceptance.sh` |

## Branch Strategy

Feature work on `jaden/big-pushg`, weekly integration to `main`.

## Commit Format

```
feat(scope): description
fix(scope): description
docs(scope): description
test(scope): description
```

Scopes: crypto, consensus, ledger, runtime, networking, rpc, programs, ai-mesh, tools, ops, docs.
