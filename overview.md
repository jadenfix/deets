# A) System overview (what we're’re building)

* **L1 base chain** (Rust): VRF-PoS + HotStuff finality, QUIC + erasure-coded fan-out, eUTxO++ ledger with **declared read/write sets** for Solana-style parallelism, WASM contracts.
* **AI service mesh** (off-chain): attested workers (SEV-SNP/TDX), deterministic inference builds, **Verifiable Compute Receipt (VCR)** posted to L1 with challenge window (TEE + KZG spot-checks + optional redundancy quorum).
* **Tokens**:

  * **SWR** (staking/governance) — secures consensus; slashable.
  * **AIC** (AI credits) — metering unit; **burned** on use; paid out to providers from escrow in stablecoins or AIC.
* **Native programs**: staking/slashing, governance, job-escrow, reputation oracle, AMM DEX.
* **Scale-out path**: L2 rollups/app-chains for app verticals; sharded execution domains with IBC-like async messages; optional external DA (Celestia/Avail) later.

---

# B) Monorepo scaffold (copy & adapt)

```
aether/
  Cargo.toml
  LICENSE
  Makefile
  README.md

  crates/
    node/                 # p2p, consensus, block production
    ledger/               # eUTxO++ store, state commitments, snapshots
    runtime/              # WASM VM, scheduler (R/W sets), cost model
    crypto/               # ed25519, BLS12-381, VRF, KES, KZG
    rpc/                  # JSON-RPC + gRPC (Firehose-style) for indexers
    types/                # canonical types, codecs
    mempool/              # gossipsub pipeline, fee market
    da/                   # shreds, RS erasure coding, DAS (optional)
    programs/             # "system" contracts compiled to WASM/native
      staking/
      governance/
      amm/
      job_escrow/
      reputation/
    verifiers/
      tee/                # SNP/TDX quote verify
      kzg/                # commitments + openings
    sdk/
      typescript/
      python/
      rust/
    tools/
      cli/                # aetherctl
      indexer/            # Postgres ingestor + GraphQL/REST
      faucet/
      keytool/
    proofs/
      tla/                # consensus safety/liveness specs
      coq/                # eUTxO++ semantics & invariants
  deploy/
    docker/               # docker-compose devnet
    k8s/                  # Helm charts, manifests
    terraform/            # multi-cloud: validator, RPC, indexer, S3/MinIO, LB
  ai-mesh/
    runtime/              # attested worker (Rust/Python), job router
    models/               # reproducible model builds, hashes, SBOMs
    receipts/             # VCR generator & validator
```

---

# C) Core specs (tight and buildable)

## C1) Consensus (VRF-PoS + HotStuff 2-chain)

* **Epoch randomness**: `η_e = H(VRF_i(η_{e-1} || e))`.
* **Leader** per slot if `U_i < τ · stake_i/Σstake` from VRF output `U_i ∈ [0,1)`.
* **Rounds per slot**: propose → prevote → precommit; aggregated BLS ≥ 2/3 stake → **final**.
* **Slashing proofs**: conflicting (header,height) signed by same key; downtime window evidence.

**Rust sketch (aggregated votes)**

```rust
fn aggregate_bls(votes: Vec<BlsSig>) -> BlsSig { blst::aggregate(&votes) }
fn has_quorum(weight: u128, total: u128) -> bool { weight * 3 >= total * 2 }
```

## C2) Networking & DA

* **Transport**: QUIC; libp2p gossipsub topics: `tx`, `header`, `vote`, `shred`.
* **Turbine shreds**: RS(n=k+r,k) coding; choose e.g. **RS(12,10)** initially.
* **Throughput sizing**: block size `B≈2MB`, slot `T_s=500ms` ⇒ leader uplink ~ `B/T_s * overhead ≈ 4–6 MB/s`.

## C3) Ledger & execution (eUTxO++ with access sets)

* **Tx declares**: `Inputs (UTxO)`, `Reads (RO objects)`, `Writes (RW accounts)`.
* **Parallelism rule**: run `tx_a, tx_b` concurrently iff
  `W(a) ∩ (W(b) ∪ R(b)) = ∅` and vice versa.
* **State commitment**: Sparse Merkle (later Verkle). Receipts have per-tx Merkle proofs.

**Scheduler predicate**

```rust
fn no_conflict(a: &Tx, b: &Tx)->bool{
  a.writes.is_disjoint(&b.writes) &&
  a.writes.is_disjoint(&b.reads)  &&
  b.writes.is_disjoint(&a.reads)
}
```

## C4) Cost & fees (deterministic)

* **Fee** `= a + b·tx_bytes + c·steps + d·mem_bytes`.
* **Rent** for on-chain state per byte per epoch; prepaid deposits exempt up to horizon `H`.
* **Congestion multipliers** per touched object (local fee markets, Solana-style).

## C5) Cryptography

* ed25519 (tx keys), **BLS12-381** (vote aggregation), **ECVRF** (leader election), **KES** (epoch-rotating validator keys), **KZG** (trace commitments), SHA-256 (hashing).

---

# D) Native programs (first set)

### D1) Staking (SWR)

* Bond/unbond (unbonding delay), delegate, rewards, slashing (`σ_d` double-sign, leak `λ` downtime).

### D2) Governance

* Param updates (fees, slot time), code upgrades (height-gated), treasury.

### D3) AMM (constant product, fixed-point)

```rust
fn swap(dx:u128, x:u128, y:u128, fee_bps:u32)->u128{
  let f = 10_000 - fee_bps as u128;
  let dx_fee = dx * f / 10_000;
  (dx_fee * y) / (x + dx_fee)
}
```

### D4) AIC Job Escrow (+ Reputation)

* `postJob(model_hash, code_hash, j_units, max_price, sla_ms, deadline_slot)`
* `acceptJob(provider, bond)`
* `submitVCR(vcr)` → start dispute timer
* `challengeVCR(proof)` → verify; slash/refund if valid
* `settle()`

VCR (on-chain struct)

```rust
struct Vcr {
  job_id: H256, provider: Addr,
  input_hash: H256, model_hash: H256, code_hash: H256, seed: u64,
  output_hash: H256,
  tee_quote: Bytes,           // attestation
  kzg_commit: Option<Bytes>,  // trace commitments
  openings: Option<Vec<Bytes>>,
  sig: Sig
}
```

---

# E) AI service mesh (verifiable providers)

* **Deterministic builds**: pinned model+ops; no nondet kernels; fixed seed/temperature.
* **TEE attestation**: SEV-SNP/TDX quotes bound to `{job_id,input_hash,model_hash,code_hash,seed}`.
* **Trace commitments** (KZG) + **random openings** for crypto-economic checks.
* **Redundant quorum** fallback (3–5 replicas; commit-reveal; slash losers).

Provider runtime (Rust/Python):

```
- attest()  -> TEE quote + PCRs
- run(job) -> outputs + trace commitments
- prove(indices) -> openings
- sign_vcr()
```

---

# F) Dev → testnet quickstart

## F1) Minimal docker devnet

`deploy/docker/docker-compose.yml` includes: 4 validators, 1 RPC, indexer(Postgres), Prometheus, Grafana, MinIO.

**Makefile**

```makefile
devnet:
\tdocker compose -f deploy/docker/docker-compose.yml up --build -d
keys:
\tcargo run -p tools/keytool -- new --out keys/
faucet:
\tcargo run -p tools/faucet -- --mint 1000000 --to $(ADDR)
```

**Run a 4-node localnet**

```bash
make devnet
cargo run -p tools/cli -- submit-tx tests/txs/genesis_stakes.json
cargo run -p tools/cli -- status
```

## F2) K8s (for real testnets)

* **Helm charts** under `deploy/k8s/`:

  * `validator` (affinity across failure domains, HPA off), `rpc`, `indexer`, `prom/grafana`, `minio`.
* **Terraform** under `deploy/terraform/`:

  * VPC, subnets (AZ-spread), node pools (validators with local NVMe, RPC with EBS gp3), NLB/Cloud Armor, S3 buckets with object lock (immutable attestation).

---

# G) SRE, security & ops (production posture)

**SLOs & budgets**

* Block finality p95 ≤ 2s; p99 ≤ 5s.
* RPC latency p95 ≤ 300ms; error budget 0.1%.
* AI job dispute resolution ≤ 24h.

**Observability**

* OpenTelemetry traces across consensus/runtime/VM.
* Prometheus dashboards: slot time, fork rate, gossip fanout, mempool depth, GPU sig-verify throughput, JU/AIC burn, dispute win rates.

**Security**

* Validators: KMS/HSM for keys; KES rotation; TMKMS-style signer.
* P2P: peer scoring, rate limits, DoS shields, Sybil resistance (stake-weighted inbound quotas).
* Upgrades: height-gated releases + “shadow fork” rehearsal; canary validators.

**Data**

* Snapshots each epoch; state sync via range proofs; indexer backfills with Firehose-like gRPC.
* Backups: S3 with object lock; WAL shipping for Postgres indexer.

---

# H) Scalability to millions/billions

1. **Throughput**: keep L1 blocks modest (2–4MB, 500ms) but parallel exec ⇒ 5–20k TPS practical on commodity validators.
2. **Horizontal scale via L2/app-chains**:

   * Standardized **light-client bridge** and **IBC-like** async messages between domains.
   * App-chains for heavy verticals (games, social, marketplaces) settle to L1.
3. **External DA (optional)**: post shreds/headers to Celestia/Avail for cheap DA; L1 verifies commitments.
4. **Edge RPC**: geo-replicated stateless RPC behind Anycast + ZK-light clients in browsers for self-verify.
5. **Payment channels**: for micro-AIC spends, use streaming channels (Perun-like) with periodic L1 settlement.
6. **Storage**: prune old states with epoch snapshots; offload artifacts to S3/IPFS with on-chain hashes.

---

# I) Performance & capacity model (sanity targets)

* **Sig-verify**: GPU ed25519 ≈ 300k–500k/s/node; if avg 2 sig/tx ⇒ verify ceiling ≈ 150–250k TPS (other limits will bind first).
* **Network**: leader egress ≈ 6 MB/s for 2MB/0.5s with 1.5× overhead; require 10GbE for validators.
* **Disk**: NVMe ≥ 1M IOPS random read for state; compaction tuned (RocksDB subcompaction, rate-limit).
* **Finality**: 2 rounds; with median RTT 100–150ms, expect p95 ~1–2s.

---

# J) Testing strategy (don’t skip)

* **Unit + property tests**: proptests for ledger invariants (no negative balances, no double spend).
* **Determinism tests**: run same tx set across N nodes; byte-identical state roots.
* **Adversarial nets**: network partitions, delayed votes, equivocation; ensure safety.
* **Fuzzers**: libFuzzer/AFL on VM opcodes, mempool path, KZG verifier.
* **Loadgen**: synthetic txs + AI jobs; SLO & saturation curves; chaos (tc netem).

---

# K) Legal & compliance (brief, non-legal)

* Treat **AIC** as utility credits; consider region-gated sales, KYC for providers receiving fiat/stable payouts, tax reporting for staking rewards.
* Export controls (models/crypto), DPAs/BAA if hosting sensitive data, TEE vendor terms.

---

# L) Concrete files to start coding (drop-in)

## L1 genesis + config (TOML)

```toml
# config/genesis.toml
[chain]
chain_id = "aether-dev-1"
slot_ms = 500
block_bytes_max = 2_000_000
epoch_slots = 43200

[consensus]
tau = 0.8    # leader rate target
quorum = "2/3"
slash_double = "0.05"
leak_downtime = "0.00001"

[fees]
a = 10_000
b = 5
c = 2
d = 1

[rent]
rho_per_byte_per_epoch = 2
horizon_epochs = 12
```

## Job Escrow ABI (JSON schema)

```json
{
  "postJob":{"inputs":["H256","H256","u64","u128","u64","u64"],"name":"postJob"},
  "acceptJob":{"inputs":["H256","Addr","u128"],"name":"acceptJob"},
  "submitVCR":{"inputs":["Bytes"],"name":"submitVCR"},
  "challengeVCR":{"inputs":["Bytes"],"name":"challengeVCR"},
  "settle":{"inputs":["H256"],"name":"settle"}
}
```

## JSON-RPC (subset)

```json
{"method":"aeth_sendRawTransaction","params":["0x..."],"id":1}
{"method":"aeth_getBlockByNumber","params":["latest",true],"id":2}
{"method":"aeth_getTransactionReceipt","params":["0x..."],"id":3}
{"method":"aeth_getStateRoot","params":["latest"],"id":4}
```

## CLI examples

```bash
# create keys
cargo run -p tools/keytool -- new --out keys/op.json
# start local 4-node
make devnet
# stake SWR
aetherctl stake --amount 100000 --from keys/op.json
# post an AI job (AIC escrow)
aetherctl job post --model-hash 0x... --code-hash 0x... \
  --jus 10 --max-price 1000 --sla 30000 --deadline +60s
# provider accepts & submits VCR
aetherctl job accept --job 0xJOB --bond 500
aetherctl job submit-vcr --job 0xJOB --vcr vcr.json
```

---

# M) Team plan & 90-day milestones

**Team**

* Core protocol (2–3), Runtime/VM (2), P2P/DA (1–2), Cryptography (1), AI mesh (2–3), SRE/DevOps (1–2), Frontend/SDK (1–2), Security (1).

**Milestones**

1. **Day 30**: devnet w/ VRF+HotStuff, eUTxO++, WASM VM, AMM & staking, explorer, faucet.
2. **Day 60**: AI mesh v1 (TEE receipts), Job Escrow, VCR verify, public testnet (≥15 validators).
3. **Day 90**: dispute game (KZG spot-checks), fee/rent economics, perf ≥ 5k TPS on testnet, docs + SDKs.

---

# N) What to implement first (order of work)

1. **Consensus & ledger core** (determinism first, perf later).
2. **Scheduler with R/W sets** + WASM runtime + cost meter.
3. **Staking/slashing** program (stake security online ASAP).
4. **Job Escrow + AIC token** (end-to-end AI job demo).
5. **TEE attestation path** (SEV-SNP) + VCR minimal.
6. **Observability + loadgen** (profiling, capacity fixes).
7. **DEX** (AIC/USDC), **Reputation** oracle, **governance**.

---