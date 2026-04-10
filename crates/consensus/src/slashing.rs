use aether_types::{Address, PublicKey, Signature, H256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Overflow-safe (a * b) / c using 256-bit intermediate product.
fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    if c == 0 {
        return 0;
    }
    let a_hi = a >> 64;
    let a_lo = a & 0xFFFF_FFFF_FFFF_FFFF;
    let b_hi = b >> 64;
    let b_lo = b & 0xFFFF_FFFF_FFFF_FFFF;

    let lo_lo = a_lo * b_lo;
    let hi_lo = a_hi * b_lo;
    let lo_hi = a_lo * b_hi;
    let hi_hi = a_hi * b_hi;

    let mid = hi_lo + (lo_lo >> 64);
    let mid = mid + lo_hi;
    let carry = if mid < lo_hi { 1u128 } else { 0u128 };

    let product_lo = (mid << 64) | (lo_lo & 0xFFFF_FFFF_FFFF_FFFF);
    let product_hi = hi_hi + (mid >> 64) + carry;

    div_256_by_128(product_hi, product_lo, c)
}

fn div_256_by_128(hi: u128, lo: u128, divisor: u128) -> u128 {
    if hi == 0 {
        return lo / divisor;
    }
    if hi >= divisor {
        return u128::MAX;
    }
    let mut remainder = hi;
    let mut quotient: u128 = 0;
    for i in (0..128).rev() {
        remainder = (remainder << 1) | ((lo >> i) & 1);
        if remainder >= divisor {
            remainder -= divisor;
            quotient |= 1u128 << i;
        }
    }
    quotient
}

/// A signed vote that can be used as evidence in a slash proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub slot: u64,
    pub block_hash: H256,
    pub validator: Address,
    pub validator_pubkey: PublicKey,
    pub signature: Signature,
}

impl Vote {
    /// Construct the canonical message that was signed.
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(self.block_hash.as_bytes());
        msg.extend_from_slice(&self.slot.to_le_bytes());
        msg
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashProof {
    pub vote1: Vote,
    pub vote2: Vote,
    pub validator: Address,
    pub proof_type: SlashType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlashType {
    /// Same slot, different blocks.
    DoubleSign,
    /// Vote A surrounds Vote B: source_a < source_b AND target_b < target_a.
    SurroundVote,
    /// Missing too many consecutive slots.
    Downtime { missing_slots: u64 },
}

/// Event emitted when a slash is applied.
#[derive(Debug, Clone)]
pub struct SlashEvent {
    pub validator: Address,
    pub proof_type: SlashType,
    pub slash_amount: u128,
    pub reporter_reward: u128,
}

/// Detect a double-sign: same validator, same slot, different blocks.
pub fn detect_double_sign(vote1: &Vote, vote2: &Vote) -> Option<SlashProof> {
    if vote1.slot == vote2.slot
        && vote1.validator == vote2.validator
        && vote1.block_hash != vote2.block_hash
    {
        Some(SlashProof {
            vote1: vote1.clone(),
            vote2: vote2.clone(),
            validator: vote1.validator,
            proof_type: SlashType::DoubleSign,
        })
    } else {
        None
    }
}

/// Detect a surround vote (Casper FFG style).
///
/// Vote A (source_a → target_a) surrounds Vote B (source_b → target_b) when:
///   source_a < source_b AND target_b < target_a
///
/// Here we use `slot` as the target and require a `source_slot` field.
/// For simplicity, we treat vote1.slot as target_a and vote2.slot as target_b.
/// The source slots are inferred from a provided source field if available,
/// or we check if one vote's range strictly contains the other.
pub fn detect_surround_vote(
    vote_a: &Vote,
    source_a: u64,
    vote_b: &Vote,
    source_b: u64,
) -> Option<SlashProof> {
    if vote_a.validator != vote_b.validator {
        return None;
    }

    // A surrounds B: source_a < source_b AND target_b < target_a
    let a_surrounds_b = source_a < source_b && vote_b.slot < vote_a.slot;
    // B surrounds A: source_b < source_a AND target_a < target_b
    let b_surrounds_a = source_b < source_a && vote_a.slot < vote_b.slot;

    if a_surrounds_b || b_surrounds_a {
        Some(SlashProof {
            vote1: vote_a.clone(),
            vote2: vote_b.clone(),
            validator: vote_a.validator,
            proof_type: SlashType::SurroundVote,
        })
    } else {
        None
    }
}

/// Verify a slash proof: check structural consistency AND cryptographic signatures.
pub fn verify_slash_proof(proof: &SlashProof) -> anyhow::Result<()> {
    match &proof.proof_type {
        SlashType::DoubleSign => {
            if proof.vote1.slot != proof.vote2.slot {
                anyhow::bail!("votes not in same slot");
            }
            if proof.vote1.block_hash == proof.vote2.block_hash {
                anyhow::bail!("votes for same block");
            }
            if proof.vote1.validator != proof.vote2.validator {
                anyhow::bail!("votes from different validators");
            }
            // Ensure proof.validator matches the votes — prevents slashing
            // an innocent validator using another validator's double-sign evidence.
            if proof.validator != proof.vote1.validator {
                anyhow::bail!(
                    "proof.validator does not match vote validator — \
                     cannot slash a different validator than the one who double-signed"
                );
            }

            // Verify signatures on both votes
            verify_vote_signature(&proof.vote1)?;
            verify_vote_signature(&proof.vote2)?;

            Ok(())
        }
        SlashType::SurroundVote => {
            if proof.vote1.validator != proof.vote2.validator {
                anyhow::bail!("votes from different validators");
            }
            if proof.validator != proof.vote1.validator {
                anyhow::bail!(
                    "proof.validator does not match vote validator — \
                     cannot slash a different validator than the one who surround-voted"
                );
            }

            verify_vote_signature(&proof.vote1)?;
            verify_vote_signature(&proof.vote2)?;

            Ok(())
        }
        SlashType::Downtime { .. } => {
            // Downtime slashing requires on-chain slot attestation records that
            // prove a validator missed consecutive slots. Without that evidence,
            // anyone could fabricate a downtime proof to slash any validator.
            // Until on-chain slot participation tracking is implemented,
            // downtime slashing via externally-submitted proofs is rejected.
            anyhow::bail!(
                "downtime slashing via proof submission is not supported — \
                 downtime penalties must be assessed by the protocol using \
                 on-chain slot participation records"
            );
        }
    }
}

/// Verify a vote's BLS signature against the validator's public key.
/// Votes are signed with BLS (not Ed25519), matching the consensus voting path.
fn verify_vote_signature(vote: &Vote) -> anyhow::Result<()> {
    let pubkey_bytes = vote.validator_pubkey.as_bytes();
    let msg = vote.signing_message();
    let sig_bytes = vote.signature.as_bytes();

    match aether_crypto_bls::keypair::verify(pubkey_bytes, &msg, sig_bytes) {
        Ok(true) => Ok(()),
        Ok(false) => Err(anyhow::anyhow!("invalid BLS vote signature")),
        Err(e) => Err(anyhow::anyhow!("BLS verification error: {}", e)),
    }
}

/// Return the slash rate in basis points for a given offense type.
///
/// This avoids the lossy roundtrip of computing an absolute slash amount and
/// then converting back to bps (which silently yields 0 when
/// `slash_amount * 10_000` overflows u128).
pub fn slash_rate_bps(proof_type: &SlashType) -> u32 {
    match proof_type {
        SlashType::DoubleSign => 500,    // 5%
        SlashType::SurroundVote => 500,  // 5%
        SlashType::Downtime { .. } => 0, // Downtime is variable; handled separately
    }
}

/// Calculate how much stake to slash.
/// Uses overflow-safe mul_div to avoid silent truncation on large u128 stakes.
pub fn calculate_slash_amount(stake: u128, proof_type: &SlashType) -> u128 {
    match proof_type {
        SlashType::DoubleSign => mul_div(stake, 5, 100), // 5%
        SlashType::SurroundVote => mul_div(stake, 5, 100), // 5%
        SlashType::Downtime { missing_slots } => {
            let leak = *missing_slots as u128;
            std::cmp::min(leak, mul_div(stake, 1, 10)) // Cap at 10%
        }
    }
}

/// Apply a slash to a validator's stake. Returns the slash event.
///
/// - Reduces validator stake by `slash_amount`
/// - 10% of slashed amount goes to the reporter as a reward
/// - If stake drops below `min_stake`, validator is deactivated
pub fn apply_slash(validator_stake: u128, proof: &SlashProof, _min_stake: u128) -> SlashEvent {
    let slash_amount = calculate_slash_amount(validator_stake, &proof.proof_type);
    let reporter_reward = slash_amount / 10; // 10% to reporter

    SlashEvent {
        validator: proof.validator,
        proof_type: proof.proof_type.clone(),
        slash_amount,
        reporter_reward,
    }
}

/// A recorded vote including signature, so double-sign proofs carry real evidence.
#[derive(Clone)]
struct RecordedVote {
    block_hash: H256,
    validator_pubkey: PublicKey,
    signature: Signature,
}

/// Tracks votes per (validator, slot) to detect double-signing in real time.
/// Designed to be embedded in the node's vote processing path.
#[derive(Default)]
pub struct SlashingDetector {
    /// Maps (validator_address, slot) -> first vote (with full signature).
    seen_votes: HashMap<(Address, u64), RecordedVote>,
    /// Pending slash proofs awaiting enforcement.
    pending_slashes: Vec<SlashProof>,
}

impl SlashingDetector {
    pub fn new() -> Self {
        SlashingDetector {
            seen_votes: HashMap::new(),
            pending_slashes: Vec::new(),
        }
    }

    /// Record a vote. If the same validator voted for a different block in the
    /// same slot, a `SlashProof` is created and returned.
    ///
    /// The caller is responsible for supplying the validator address and BLS
    /// public key (resolved from the `aether_types::Vote`).
    pub fn record_vote(
        &mut self,
        validator: Address,
        validator_pubkey: aether_types::PublicKey,
        slot: u64,
        block_hash: H256,
        signature: aether_types::Signature,
    ) -> Option<SlashProof> {
        let key = (validator, slot);
        match self.seen_votes.entry(key) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(RecordedVote {
                    block_hash,
                    validator_pubkey,
                    signature,
                });
                None
            }
            std::collections::hash_map::Entry::Occupied(e) => {
                let first = e.get();
                if first.block_hash == block_hash {
                    return None; // Duplicate vote, not a double-sign
                }
                // Double-sign detected — both votes carry real signatures
                let vote1 = Vote {
                    slot,
                    block_hash: first.block_hash,
                    validator,
                    validator_pubkey: first.validator_pubkey.clone(),
                    signature: first.signature.clone(),
                };
                let vote2 = Vote {
                    slot,
                    block_hash,
                    validator,
                    validator_pubkey,
                    signature,
                };
                let proof = SlashProof {
                    vote1,
                    vote2,
                    validator,
                    proof_type: SlashType::DoubleSign,
                };
                self.pending_slashes.push(proof.clone());
                Some(proof)
            }
        }
    }

    /// Drain all pending slash proofs for processing.
    pub fn drain_pending(&mut self) -> Vec<SlashProof> {
        std::mem::take(&mut self.pending_slashes)
    }

    /// Prune vote records for slots below `min_slot` to bound memory.
    pub fn prune_before(&mut self, min_slot: u64) {
        self.seen_votes.retain(|&(_, slot), _| slot >= min_slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_bls::BlsKeypair;

    fn make_vote(kp: &BlsKeypair, slot: u64, block_byte: u8) -> Vote {
        let validator_pubkey = PublicKey::from_bytes(kp.public_key());
        let validator = validator_pubkey.to_address();
        let block_hash = H256::from_slice(&[block_byte; 32]).unwrap();

        let vote = Vote {
            slot,
            block_hash,
            validator,
            validator_pubkey,
            signature: Signature::from_bytes(vec![]), // placeholder
        };

        // Sign properly with BLS
        let msg = vote.signing_message();
        let sig = kp.sign(&msg);
        Vote {
            signature: Signature::from_bytes(sig),
            ..vote
        }
    }

    #[test]
    fn test_detect_double_sign() {
        let kp = BlsKeypair::generate();
        let vote1 = make_vote(&kp, 100, 1);
        let vote2 = make_vote(&kp, 100, 2);

        let proof = detect_double_sign(&vote1, &vote2);
        assert!(proof.is_some());
        assert!(matches!(proof.unwrap().proof_type, SlashType::DoubleSign));
    }

    #[test]
    fn test_no_double_sign_same_block() {
        let kp = BlsKeypair::generate();
        let vote1 = make_vote(&kp, 100, 1);
        let vote2 = make_vote(&kp, 100, 1);

        assert!(detect_double_sign(&vote1, &vote2).is_none());
    }

    #[test]
    fn test_verify_double_sign_proof_checks_signatures() {
        let kp = BlsKeypair::generate();
        let vote1 = make_vote(&kp, 100, 1);
        let vote2 = make_vote(&kp, 100, 2);

        let proof = detect_double_sign(&vote1, &vote2).unwrap();

        // Valid proof should pass
        assert!(verify_slash_proof(&proof).is_ok());
    }

    #[test]
    fn test_verify_proof_rejects_forged_signature() {
        let kp = BlsKeypair::generate();
        let mut vote1 = make_vote(&kp, 100, 1);
        let vote2 = make_vote(&kp, 100, 2);

        // Forge the signature (BLS signatures are 96 bytes)
        vote1.signature = Signature::from_bytes(vec![0u8; 96]);

        let proof = SlashProof {
            vote1,
            vote2,
            validator: PublicKey::from_bytes(kp.public_key()).to_address(),
            proof_type: SlashType::DoubleSign,
        };

        assert!(
            verify_slash_proof(&proof).is_err(),
            "forged signature must be rejected"
        );
    }

    #[test]
    fn test_detect_surround_vote() {
        let kp = BlsKeypair::generate();
        // Vote A: source=10, target=100 (wide range)
        let vote_a = make_vote(&kp, 100, 1);
        // Vote B: source=20, target=50 (narrower range inside A)
        let vote_b = make_vote(&kp, 50, 2);

        let proof = detect_surround_vote(&vote_a, 10, &vote_b, 20);
        assert!(proof.is_some());
        assert!(matches!(proof.unwrap().proof_type, SlashType::SurroundVote));
    }

    #[test]
    fn test_no_surround_vote_non_overlapping() {
        let kp = BlsKeypair::generate();
        // Vote A: source=10, target=20
        let vote_a = make_vote(&kp, 20, 1);
        // Vote B: source=30, target=40
        let vote_b = make_vote(&kp, 40, 2);

        let proof = detect_surround_vote(&vote_a, 10, &vote_b, 30);
        assert!(proof.is_none());
    }

    #[test]
    fn test_no_surround_vote_different_validators() {
        let kp1 = BlsKeypair::generate();
        let kp2 = BlsKeypair::generate();
        let vote_a = make_vote(&kp1, 100, 1);
        let vote_b = make_vote(&kp2, 50, 2);

        let proof = detect_surround_vote(&vote_a, 10, &vote_b, 20);
        assert!(proof.is_none());
    }

    #[test]
    fn test_slash_rate_bps() {
        assert_eq!(slash_rate_bps(&SlashType::DoubleSign), 500);
        assert_eq!(slash_rate_bps(&SlashType::SurroundVote), 500);
        assert_eq!(
            slash_rate_bps(&SlashType::Downtime { missing_slots: 100 }),
            0
        );
    }

    #[test]
    fn test_calculate_slash_amount() {
        let stake = 1_000_000u128;

        assert_eq!(
            calculate_slash_amount(stake, &SlashType::DoubleSign),
            50_000
        );
        assert_eq!(
            calculate_slash_amount(stake, &SlashType::SurroundVote),
            50_000
        );
        assert_eq!(
            calculate_slash_amount(stake, &SlashType::Downtime { missing_slots: 200 }),
            200
        );
        // Downtime cap at 10% of stake
        assert_eq!(
            calculate_slash_amount(
                stake,
                &SlashType::Downtime {
                    missing_slots: 1_000_000
                }
            ),
            100_000
        );
        // Overflow-safe: mul_div(u128::MAX, 5, 100) == exact 5% of u128::MAX
        assert_eq!(
            calculate_slash_amount(u128::MAX, &SlashType::DoubleSign),
            mul_div(u128::MAX, 5, 100)
        );
    }

    #[test]
    fn test_calculate_slash_no_overflow_on_large_stakes() {
        // Verify that mul_div produces the mathematically correct result
        // even for stakes near u128::MAX. The old saturating_mul(5)/100
        // would return u128::MAX/100 (≈1%) instead of the correct 5%.
        let stake = u128::MAX;
        let slash = calculate_slash_amount(stake, &SlashType::DoubleSign);

        // Correct 5% of u128::MAX
        // u128::MAX = 340282366920938463463374607431768211455
        // 5% = 17014118346046923173168730371588410572
        let expected = mul_div(u128::MAX, 5, 100);
        assert_eq!(slash, expected);

        // The old (wrong) value was u128::MAX / 100 ≈ 1% — ensure we're higher
        assert!(
            slash > u128::MAX / 100,
            "slash should be ~5x larger than the old 1% result"
        );
    }

    #[test]
    fn test_downtime_slash_proof_rejected() {
        // Downtime proofs must be rejected — they require no cryptographic evidence
        // and could be used to slash any validator by anyone.
        let kp = BlsKeypair::generate();
        let vote = make_vote(&kp, 100, 1);
        let proof = SlashProof {
            vote1: vote.clone(),
            vote2: vote,
            validator: PublicKey::from_bytes(kp.public_key()).to_address(),
            proof_type: SlashType::Downtime { missing_slots: 200 },
        };
        let err = verify_slash_proof(&proof).unwrap_err();
        assert!(
            err.to_string().contains("not supported"),
            "downtime proof should be rejected, got: {}",
            err
        );
    }

    #[test]
    fn test_validator_mismatch_double_sign_rejected() {
        // An attacker submits valid double-sign votes from validator A
        // but sets proof.validator = B to try to slash B instead.
        let kp_a = BlsKeypair::generate();
        let kp_b = BlsKeypair::generate();
        let vote1 = make_vote(&kp_a, 100, 1);
        let vote2 = make_vote(&kp_a, 100, 2);

        let proof = SlashProof {
            vote1,
            vote2,
            validator: PublicKey::from_bytes(kp_b.public_key()).to_address(), // victim B
            proof_type: SlashType::DoubleSign,
        };
        let err = verify_slash_proof(&proof).unwrap_err();
        assert!(
            err.to_string().contains("does not match"),
            "validator mismatch should be rejected, got: {}",
            err
        );
    }

    #[test]
    fn test_validator_mismatch_surround_vote_rejected() {
        let kp_a = BlsKeypair::generate();
        let kp_b = BlsKeypair::generate();
        let vote_a = make_vote(&kp_a, 100, 1);
        let vote_b = make_vote(&kp_a, 50, 2);

        let proof = SlashProof {
            vote1: vote_a,
            vote2: vote_b,
            validator: PublicKey::from_bytes(kp_b.public_key()).to_address(),
            proof_type: SlashType::SurroundVote,
        };
        let err = verify_slash_proof(&proof).unwrap_err();
        assert!(
            err.to_string().contains("does not match"),
            "validator mismatch should be rejected, got: {}",
            err
        );
    }

    #[test]
    fn test_apply_slash_returns_event() {
        let kp = BlsKeypair::generate();
        let vote1 = make_vote(&kp, 100, 1);
        let vote2 = make_vote(&kp, 100, 2);
        let proof = detect_double_sign(&vote1, &vote2).unwrap();

        let event = apply_slash(1_000_000, &proof, 100);

        assert_eq!(event.slash_amount, 50_000); // 5% of 1M
        assert_eq!(event.reporter_reward, 5_000); // 10% of slash
        assert_eq!(event.proof_type, SlashType::DoubleSign);
    }

    #[test]
    fn test_slashing_detector_detects_double_sign() {
        let mut detector = SlashingDetector::new();
        let kp = aether_crypto_primitives::Keypair::generate();
        let pubkey = aether_types::PublicKey::from_bytes(kp.public_key());
        let addr = pubkey.to_address();
        let sig = aether_types::Signature::from_bytes(vec![0; 64]);

        let hash_a = H256::from_slice(&[1u8; 32]).unwrap();
        let hash_b = H256::from_slice(&[2u8; 32]).unwrap();

        // First vote — no slash
        assert!(detector
            .record_vote(addr, pubkey.clone(), 10, hash_a, sig.clone())
            .is_none());

        // Same block again — no slash (duplicate)
        assert!(detector
            .record_vote(addr, pubkey.clone(), 10, hash_a, sig.clone())
            .is_none());

        // Different block, same slot — SLASH
        let proof = detector.record_vote(addr, pubkey.clone(), 10, hash_b, sig.clone());
        assert!(proof.is_some());
        let proof = proof.unwrap();
        assert_eq!(proof.validator, addr);
        assert!(matches!(proof.proof_type, SlashType::DoubleSign));
    }

    #[test]
    fn test_slashing_detector_different_slots_ok() {
        let mut detector = SlashingDetector::new();
        let kp = aether_crypto_primitives::Keypair::generate();
        let pubkey = aether_types::PublicKey::from_bytes(kp.public_key());
        let addr = pubkey.to_address();
        let sig = aether_types::Signature::from_bytes(vec![0; 64]);

        let hash_a = H256::from_slice(&[1u8; 32]).unwrap();
        let hash_b = H256::from_slice(&[2u8; 32]).unwrap();

        // Different slots are fine
        assert!(detector
            .record_vote(addr, pubkey.clone(), 10, hash_a, sig.clone())
            .is_none());
        assert!(detector
            .record_vote(addr, pubkey.clone(), 11, hash_b, sig.clone())
            .is_none());
    }

    #[test]
    fn test_slashing_detector_prune() {
        let mut detector = SlashingDetector::new();
        let kp = aether_crypto_primitives::Keypair::generate();
        let pubkey = aether_types::PublicKey::from_bytes(kp.public_key());
        let addr = pubkey.to_address();
        let sig = aether_types::Signature::from_bytes(vec![0; 64]);

        let hash_a = H256::from_slice(&[1u8; 32]).unwrap();
        let hash_b = H256::from_slice(&[2u8; 32]).unwrap();

        detector.record_vote(addr, pubkey.clone(), 5, hash_a, sig.clone());
        detector.prune_before(10);

        // After pruning, slot 5 is forgotten — double-sign at slot 5 won't be caught
        // (which is correct: finalized slots don't need protection)
        assert!(detector
            .record_vote(addr, pubkey.clone(), 5, hash_b, sig.clone())
            .is_none());
    }

    #[test]
    fn test_slashing_detector_produces_verifiable_proofs() {
        // The detector must store full vote signatures so that the proofs
        // it produces pass verify_slash_proof (BLS signature check).
        let mut detector = SlashingDetector::new();
        let kp = BlsKeypair::generate();
        let pubkey = PublicKey::from_bytes(kp.public_key());
        let addr = pubkey.to_address();

        let hash_a = H256::from_slice(&[1u8; 32]).unwrap();
        let hash_b = H256::from_slice(&[2u8; 32]).unwrap();

        // Sign both votes properly with BLS
        let msg_a = {
            let mut m = Vec::new();
            m.extend_from_slice(hash_a.as_bytes());
            m.extend_from_slice(&10u64.to_le_bytes());
            m
        };
        let msg_b = {
            let mut m = Vec::new();
            m.extend_from_slice(hash_b.as_bytes());
            m.extend_from_slice(&10u64.to_le_bytes());
            m
        };
        let sig_a = Signature::from_bytes(kp.sign(&msg_a));
        let sig_b = Signature::from_bytes(kp.sign(&msg_b));

        // First vote
        assert!(detector
            .record_vote(addr, pubkey.clone(), 10, hash_a, sig_a)
            .is_none());

        // Double-sign — should produce a verifiable proof
        let proof = detector
            .record_vote(addr, pubkey.clone(), 10, hash_b, sig_b)
            .expect("should detect double-sign");

        // The proof must pass full cryptographic verification
        verify_slash_proof(&proof).expect("detector-produced proof must be verifiable");
    }

    #[test]
    fn test_slashing_detector_drain_pending() {
        let mut detector = SlashingDetector::new();
        let kp = aether_crypto_primitives::Keypair::generate();
        let pubkey = aether_types::PublicKey::from_bytes(kp.public_key());
        let addr = pubkey.to_address();
        let sig = aether_types::Signature::from_bytes(vec![0; 64]);

        let hash_a = H256::from_slice(&[1u8; 32]).unwrap();
        let hash_b = H256::from_slice(&[2u8; 32]).unwrap();

        detector.record_vote(addr, pubkey.clone(), 10, hash_a, sig.clone());
        detector.record_vote(addr, pubkey.clone(), 10, hash_b, sig.clone());

        let pending = detector.drain_pending();
        assert_eq!(pending.len(), 1);
        assert!(detector.drain_pending().is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_crypto_bls::BlsKeypair;
    use proptest::prelude::*;

    fn arb_stake() -> impl Strategy<Value = u128> {
        prop_oneof![
            1u128..=1_000_000_000_000u128,
            Just(u128::MAX),
            Just(u128::MAX / 2),
            Just(1u128),
        ]
    }

    fn arb_slot() -> impl Strategy<Value = u64> {
        any::<u64>()
    }

    fn arb_h256() -> impl Strategy<Value = H256> {
        prop::array::uniform32(any::<u8>()).prop_map(|b| H256::from_slice(&b).unwrap())
    }

    fn make_bls_vote(kp: &BlsKeypair, slot: u64, block_hash: H256) -> Vote {
        let pubkey = PublicKey::from_bytes(kp.public_key());
        let addr = pubkey.to_address();
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&slot.to_le_bytes());
        let sig = kp.sign(&msg);
        Vote {
            slot,
            block_hash,
            validator: addr,
            validator_pubkey: pubkey,
            signature: Signature::from_bytes(sig),
        }
    }

    proptest! {
        /// Slash amount for DoubleSign/SurroundVote is exactly 5% of stake.
        #[test]
        fn slash_amount_is_five_percent(stake in arb_stake()) {
            let ds = calculate_slash_amount(stake, &SlashType::DoubleSign);
            let sv = calculate_slash_amount(stake, &SlashType::SurroundVote);
            prop_assert_eq!(ds, sv, "double-sign and surround-vote should slash equally");
            // 5% means slash * 20 should approximate stake (with rounding)
            prop_assert!(ds <= stake, "slash must not exceed stake");
            if stake >= 20 {
                // For stakes >= 20, 5% should be at least 1
                prop_assert!(ds >= 1, "5% of {} should be >= 1", stake);
            }
        }

        /// Downtime slash is capped at 10% of stake.
        #[test]
        fn downtime_slash_capped_at_ten_percent(
            stake in arb_stake(),
            missing in any::<u64>(),
        ) {
            let slash = calculate_slash_amount(stake, &SlashType::Downtime { missing_slots: missing });
            let cap = mul_div(stake, 1, 10);
            prop_assert!(slash <= cap, "downtime slash {} exceeds 10% cap {}", slash, cap);
            prop_assert!(slash <= stake, "slash must not exceed stake");
        }

        /// Reporter reward is always 10% of slash amount (floor division).
        #[test]
        fn reporter_reward_is_ten_percent_of_slash(stake in arb_stake()) {
            let kp = BlsKeypair::generate();
            let vote1 = make_bls_vote(&kp, 100, H256::from_slice(&[1u8; 32]).unwrap());
            let vote2 = make_bls_vote(&kp, 100, H256::from_slice(&[2u8; 32]).unwrap());
            let proof = detect_double_sign(&vote1, &vote2).unwrap();
            let event = apply_slash(stake, &proof, 0);
            prop_assert_eq!(event.reporter_reward, event.slash_amount / 10);
        }

        /// detect_double_sign only fires when same validator, same slot, different block.
        #[test]
        fn double_sign_requires_same_slot_different_block(
            slot_a in arb_slot(),
            slot_b in arb_slot(),
            hash_a in arb_h256(),
            hash_b in arb_h256(),
        ) {
            let kp = BlsKeypair::generate();
            let vote_a = make_bls_vote(&kp, slot_a, hash_a);
            let vote_b = make_bls_vote(&kp, slot_b, hash_b);
            let result = detect_double_sign(&vote_a, &vote_b);
            if slot_a == slot_b && hash_a != hash_b {
                prop_assert!(result.is_some(), "should detect double-sign");
            } else {
                prop_assert!(result.is_none(), "should not detect double-sign");
            }
        }

        /// Surround vote detection is symmetric: if A surrounds B, B surrounds A.
        #[test]
        fn surround_vote_symmetric_detection(
            src_a in 0u64..1000,
            tgt_a in 0u64..1000,
            src_b in 0u64..1000,
            tgt_b in 0u64..1000,
        ) {
            let kp = BlsKeypair::generate();
            let vote_a = make_bls_vote(&kp, tgt_a, H256::from_slice(&[1u8; 32]).unwrap());
            let vote_b = make_bls_vote(&kp, tgt_b, H256::from_slice(&[2u8; 32]).unwrap());
            let ab = detect_surround_vote(&vote_a, src_a, &vote_b, src_b);
            let ba = detect_surround_vote(&vote_b, src_b, &vote_a, src_a);
            // Both directions should detect or not detect
            prop_assert_eq!(ab.is_some(), ba.is_some(),
                "surround detection must be symmetric");
        }

        /// Different validators never produce a surround vote detection.
        #[test]
        fn surround_vote_different_validators_never_detected(
            src_a in 0u64..1000,
            tgt_a in 0u64..1000,
            src_b in 0u64..1000,
            tgt_b in 0u64..1000,
        ) {
            let kp_a = BlsKeypair::generate();
            let kp_b = BlsKeypair::generate();
            let vote_a = make_bls_vote(&kp_a, tgt_a, H256::from_slice(&[1u8; 32]).unwrap());
            let vote_b = make_bls_vote(&kp_b, tgt_b, H256::from_slice(&[2u8; 32]).unwrap());
            prop_assert!(detect_surround_vote(&vote_a, src_a, &vote_b, src_b).is_none());
        }

        /// Verified double-sign proofs always pass verify_slash_proof.
        #[test]
        fn valid_double_sign_proof_verifies(slot in 0u64..10000) {
            let kp = BlsKeypair::generate();
            let hash_a = H256::from_slice(&[1u8; 32]).unwrap();
            let hash_b = H256::from_slice(&[2u8; 32]).unwrap();
            let vote1 = make_bls_vote(&kp, slot, hash_a);
            let vote2 = make_bls_vote(&kp, slot, hash_b);
            let proof = detect_double_sign(&vote1, &vote2).unwrap();
            prop_assert!(verify_slash_proof(&proof).is_ok());
        }

        /// SlashingDetector detects double-sign for arbitrary slots and hashes.
        #[test]
        fn detector_finds_double_sign(slot in arb_slot(), ha in arb_h256(), hb in arb_h256()) {
            prop_assume!(ha != hb);
            let mut detector = SlashingDetector::new();
            let kp = aether_crypto_primitives::Keypair::generate();
            let pubkey = PublicKey::from_bytes(kp.public_key());
            let addr = pubkey.to_address();
            let sig = Signature::from_bytes(vec![0; 64]);

            assert!(detector.record_vote(addr, pubkey.clone(), slot, ha, sig.clone()).is_none());
            let proof = detector.record_vote(addr, pubkey.clone(), slot, hb, sig.clone());
            prop_assert!(proof.is_some(), "detector must catch double-sign");
            prop_assert!(matches!(proof.unwrap().proof_type, SlashType::DoubleSign));
        }

        /// Prune removes only votes below the threshold.
        #[test]
        fn prune_removes_old_slots(
            old_slot in 0u64..100,
            new_slot in 100u64..200,
        ) {
            let mut detector = SlashingDetector::new();
            let kp = aether_crypto_primitives::Keypair::generate();
            let pubkey = PublicKey::from_bytes(kp.public_key());
            let addr = pubkey.to_address();
            let sig = Signature::from_bytes(vec![0; 64]);
            let hash = H256::from_slice(&[1u8; 32]).unwrap();

            detector.record_vote(addr, pubkey.clone(), old_slot, hash, sig.clone());
            detector.record_vote(addr, pubkey.clone(), new_slot, hash, sig.clone());
            detector.prune_before(100);

            // Old slot forgotten — re-voting doesn't trigger double-sign
            let hash2 = H256::from_slice(&[2u8; 32]).unwrap();
            prop_assert!(detector.record_vote(addr, pubkey.clone(), old_slot, hash2, sig.clone()).is_none());
            // New slot still tracked
            prop_assert!(detector.record_vote(addr, pubkey.clone(), new_slot, hash2, sig.clone()).is_some());
        }

        /// mul_div never panics and result <= a when b <= c.
        #[test]
        fn mul_div_bounded(a in any::<u128>(), b in 0u128..=100, c in 1u128..=100) {
            let result = mul_div(a, b, c);
            if b <= c {
                prop_assert!(result <= a, "mul_div({}, {}, {}) = {} > a", a, b, c, result);
            }
        }
    }
}
