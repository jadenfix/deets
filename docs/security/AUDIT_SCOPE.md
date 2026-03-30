# Aether Security Audit Scope

## Overview

This document defines the scope, priorities, and recommended firms for security audits of the Aether L1 blockchain. The audit should be conducted before any incentivized testnet or mainnet launch.

## In-Scope Crates (Priority Order)

### Tier 1: Critical (Consensus + Crypto)
| Crate | LOC | Key Risk | Files |
|-------|-----|----------|-------|
| `aether-consensus` | ~800 | Byzantine safety, finality correctness | `hotstuff.rs`, `pacemaker.rs`, `slashing.rs`, `vrf_pos.rs` |
| `aether-crypto-vrf` | ~550 | Leader election predictability | `ecvrf.rs` (ECVRF-EDWARDS25519-SHA512-ELL2) |
| `aether-crypto-bls` | ~300 | Vote aggregation soundness | `lib.rs` (BLS12-381 via blst) |
| `aether-crypto-kzg` | ~450 | AI trace verification | `commitment.rs` (pairing-based KZG) |
| `aether-crypto-kes` | ~350 | Forward secrecy, key erasure | `evolution.rs`, `signature.rs` |

### Tier 2: High (Execution + State)
| Crate | LOC | Key Risk | Files |
|-------|-----|----------|-------|
| `aether-runtime` | ~400 | WASM sandbox escape, gas metering bypass | `vm.rs` (Wasmtime host functions) |
| `aether-ledger` | ~500 | Double-spend, balance overflow | `state.rs` (eUTxO++ execution) |
| `aether-state-merkle` | ~350 | State proof forgery | `tree.rs`, `proof.rs` (Sparse Merkle Tree) |
| `aether-mempool` | ~300 | DoS via mempool flooding | `pool.rs` (nonce tracking, rate limiting) |

### Tier 3: Medium (Networking + Programs)
| Crate | LOC | Key Risk | Files |
|-------|-----|----------|-------|
| `aether-p2p` | ~400 | Eclipse attacks, message injection | `network.rs` (libp2p), `peer_diversity.rs` |
| `aether-programs-staking` | ~400 | Slashing griefing, stake manipulation | `state.rs` |
| `aether-programs-job-escrow` | ~300 | Payment theft, dispute manipulation | `lib.rs` |
| `aether-verifiers-vcr` | ~350 | False verification acceptance | `lib.rs` |

## Threat Model Mapping

Each threat from `THREAT_MODEL.md` maps to specific code:

| Threat | Severity | Mitigation Code |
|--------|----------|-----------------|
| T1: Validator impersonation | High | `consensus/slashing.rs:verify_vote_signature()` |
| T2: Block tampering | High | `types/block.rs:hash()`, `ledger/state.rs` |
| T3: Double-spend | High | `mempool/pool.rs:nonce tracking`, `ledger/state.rs:apply_tx` |
| T4: VRF grinding | High | `crypto/vrf/ecvrf.rs:prove()` (ECVRF with Elligator2) |
| T5: BLS rogue key | High | `crypto/bls/lib.rs` (proof-of-possession via blst) |
| T6: WASM escape | High | `runtime/vm.rs` (Wasmtime sandboxing, fuel metering) |
| T7: Eclipse attack | Medium | `p2p/peer_diversity.rs:PeerDiversityGuard` |
| T8: Mempool DoS | Medium | `mempool/pool.rs:check_rate_limit()` |
| T9: MEV extraction | Medium | Not yet mitigated (Phase 5) |
| T10: State proof forgery | High | `state/merkle/proof.rs:verify()` |

## Recommended Audit Firms

1. **Trail of Bits** — Specializes in consensus protocols, cryptography, and smart contract security. Best for Tier 1 (consensus + crypto).

2. **OtterSec** — Strong in Rust blockchain audits (Solana ecosystem). Best for Tier 2 (runtime, state management).

3. **Zellic** — Focuses on DeFi and bridge security. Best for Tier 3 (staking, job escrow, networking).

## Estimated Timeline

| Phase | Duration | Budget |
|-------|----------|--------|
| Tier 1 audit (consensus + crypto) | 4-6 weeks | $200K-$300K |
| Tier 2 audit (execution + state) | 3-4 weeks | $150K-$200K |
| Tier 3 audit (networking + programs) | 2-3 weeks | $100K-$150K |
| Remediation + re-audit | 2-3 weeks | $50K-$100K |
| **Total** | **11-16 weeks** | **$500K-$750K** |

## Pre-Audit Checklist

- [x] `cargo deny check bans sources` passes (supply chain)
- [x] `cargo deny check advisories` reports known issues (tracked)
- [x] 5 fuzzing targets created and compilable
- [x] 7 property-based tests (proptest) covering core invariants
- [x] THREAT_MODEL.md with 23 identified threats
- [x] REMOTE_SIGNER.md with HSM architecture design
- [ ] All fuzzing targets run for 24h with no crashes
- [ ] External penetration test of RPC endpoints
- [ ] TLA+ model checking run completed
- [ ] Code coverage >70% on Tier 1 crates

## Supply Chain Findings (cargo-deny)

Current advisories detected (informational — to be addressed before audit):
- `bincode`: unmaintained (consider migration to `bincode2` or `postcard`)
- `bytes`: integer overflow in `BytesMut::reserve` (update to patched version)
- `ring` < 0.17: unmaintained (already using 0.17 via libp2p)
- `wasmtime`: multiple sandbox issues (update to latest)
- `quinn`: DoS in endpoints (update to latest)

These should be resolved (dependency updates) before engaging auditors.
