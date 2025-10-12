// ============================================================================
// AETHER CRYPTO BLS - BLS12-381 Signature Aggregation
// ============================================================================
// PURPOSE: Aggregate thousands of validator votes into single signature
//
// ALGORITHM: BLS (Boneh-Lynn-Shacham) signatures on BLS12-381 curve
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    BLS AGGREGATION                                │
// ├──────────────────────────────────────────────────────────────────┤
// │  Individual Votes  →  BLS Sign (per validator)  →  Gossip 'vote' │
// │         ↓                                             ↓           │
// │  Vote Collector  →  Aggregate Signatures  →  Single Aggregated   │
// │         ↓                                             ↓           │
// │  Aggregate Verify (batch)  →  Quorum Check (≥2/3 stake)          │
// │         ↓                                             ↓           │
// │  Store in Block  →  Finality Proof                                │
// └──────────────────────────────────────────────────────────────────┘
//
// BLS ADVANTAGE:
// - Aggregate 1000 signatures → 1 signature (96 bytes)
// - Aggregate 1000 pubkeys → 1 pubkey (48 bytes)
// - Single pairing verification for all
//
// VOTE STRUCTURE:
// ```
// struct Vote:
//     block_hash: H256
//     slot: u64
//     validator_pubkey: BlsPublicKey
//     signature: BlsSignature
//     stake: u128
// ```
//
// AGGREGATION:
// ```
// fn aggregate_votes(votes: Vec<Vote>) -> AggregatedVote:
//     // Aggregate signatures
//     sigs = votes.map(|v| v.signature)
//     agg_sig = bls_aggregate_signatures(sigs)
//
//     // Aggregate pubkeys (for verification)
//     pubkeys = votes.map(|v| v.validator_pubkey)
//     agg_pubkey = bls_aggregate_pubkeys(pubkeys)
//
//     // Sum stake
//     total_stake = sum(votes.map(|v| v.stake))
//
//     return AggregatedVote {
//         block_hash: votes[0].block_hash,
//         slot: votes[0].slot,
//         aggregated_signature: agg_sig,
//         aggregated_pubkey: agg_pubkey,
//         total_stake: total_stake,
//         signers: votes.map(|v| v.validator_pubkey)
//     }
// ```
//
// VERIFICATION:
// ```
// fn verify_aggregated_vote(agg_vote, message) -> bool:
//     return bls_verify(
//         agg_vote.aggregated_pubkey,
//         message,
//         agg_vote.aggregated_signature
//     )
// ```
//
// OPTIMIZATIONS:
// - Batch verify multiple aggregated votes
// - Precompute pairing elements
// - Parallel signature aggregation
//
// SECURITY:
// - Rogue key attack prevention (proof-of-possession)
// - Slashing for conflicting signatures
//
// OUTPUTS:
// - Aggregated signature → Block finality proof
// - Verification result → Consensus state transition
// - Signer list → Reward distribution
// ============================================================================

pub mod aggregate;
pub mod keypair;
pub mod verify;

pub use aggregate::aggregate_signatures;
pub use keypair::BlsKeypair;
pub use verify::verify_aggregated;
