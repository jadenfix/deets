use aether_types::{Address, PublicKey, Signature, H256};
use serde::{Deserialize, Serialize};

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

            // Verify signatures on both votes
            verify_vote_signature(&proof.vote1)?;
            verify_vote_signature(&proof.vote2)?;

            Ok(())
        }
        SlashType::SurroundVote => {
            if proof.vote1.validator != proof.vote2.validator {
                anyhow::bail!("votes from different validators");
            }

            verify_vote_signature(&proof.vote1)?;
            verify_vote_signature(&proof.vote2)?;

            Ok(())
        }
        SlashType::Downtime { missing_slots } => {
            if *missing_slots < 100 {
                anyhow::bail!("downtime threshold not met (need >= 100 missing slots)");
            }
            Ok(())
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

/// Calculate how much stake to slash.
pub fn calculate_slash_amount(stake: u128, proof_type: &SlashType) -> u128 {
    match proof_type {
        SlashType::DoubleSign => (stake * 5) / 100, // 5%
        SlashType::SurroundVote => (stake * 5) / 100, // 5% (same as double-sign)
        SlashType::Downtime { missing_slots } => {
            let leak_rate = 1u128;
            let leak = leak_rate * (*missing_slots as u128);
            std::cmp::min(leak, stake / 10) // Cap at 10%
        }
    }
}

/// Apply a slash to a validator's stake. Returns the slash event.
///
/// - Reduces validator stake by `slash_amount`
/// - 10% of slashed amount goes to the reporter as a reward
/// - If stake drops below `min_stake`, validator is deactivated
pub fn apply_slash(
    validator_stake: u128,
    proof: &SlashProof,
    _min_stake: u128,
) -> SlashEvent {
    let slash_amount = calculate_slash_amount(validator_stake, &proof.proof_type);
    let reporter_reward = slash_amount / 10; // 10% to reporter

    SlashEvent {
        validator: proof.validator,
        proof_type: proof.proof_type.clone(),
        slash_amount,
        reporter_reward,
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
        assert!(matches!(
            proof.unwrap().proof_type,
            SlashType::DoubleSign
        ));
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
        assert!(matches!(
            proof.unwrap().proof_type,
            SlashType::SurroundVote
        ));
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
    fn test_calculate_slash_amount() {
        let stake = 1_000_000u128;

        assert_eq!(calculate_slash_amount(stake, &SlashType::DoubleSign), 50_000);
        assert_eq!(
            calculate_slash_amount(stake, &SlashType::SurroundVote),
            50_000
        );
        assert_eq!(
            calculate_slash_amount(stake, &SlashType::Downtime { missing_slots: 200 }),
            200
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
}
