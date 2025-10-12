use aether_types::{address::Address, crypto::Signature, hash::H256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub slot: u64,
    pub block_hash: H256,
    pub validator: Address,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashProof {
    pub vote1: Vote,
    pub vote2: Vote,
    pub validator: Address,
    pub proof_type: SlashType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashType {
    DoubleSign,
    Downtime { missing_slots: u64 },
}

pub fn detect_double_sign(vote1: &Vote, vote2: &Vote) -> Option<SlashProof> {
    if vote1.slot == vote2.slot
        && vote1.validator == vote2.validator
        && vote1.block_hash != vote2.block_hash
    {
        Some(SlashProof {
            vote1: vote1.clone(),
            vote2: vote2.clone(),
            validator: vote1.validator.clone(),
            proof_type: SlashType::DoubleSign,
        })
    } else {
        None
    }
}

pub fn verify_slash_proof(proof: &SlashProof) -> anyhow::Result<()> {
    use aether_types::crypto::verify;

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

            Ok(())
        }
        SlashType::Downtime { missing_slots } => {
            if *missing_slots < 100 {
                anyhow::bail!("downtime threshold not met");
            }
            Ok(())
        }
    }
}

pub fn calculate_slash_amount(
    stake: u128,
    proof_type: &SlashType,
) -> u128 {
    match proof_type {
        SlashType::DoubleSign => {
            (stake * 5) / 100
        }
        SlashType::Downtime { missing_slots } => {
            let leak_rate = 1u128;
            let leak = leak_rate * (*missing_slots as u128);
            std::cmp::min(leak, stake / 10)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::crypto::{sign, Keypair};

    #[test]
    fn test_detect_double_sign() {
        let kp = Keypair::generate();
        let validator = Address::from_pubkey(&kp.public);

        let vote1 = Vote {
            slot: 100,
            block_hash: H256::from_slice(&[1u8; 32]),
            validator: validator.clone(),
            signature: sign(&kp.secret, b"vote1"),
        };

        let vote2 = Vote {
            slot: 100,
            block_hash: H256::from_slice(&[2u8; 32]),
            validator: validator.clone(),
            signature: sign(&kp.secret, b"vote2"),
        };

        let proof = detect_double_sign(&vote1, &vote2);
        assert!(proof.is_some());

        let proof = proof.unwrap();
        assert_eq!(proof.validator, validator);
        assert!(matches!(proof.proof_type, SlashType::DoubleSign));
    }

    #[test]
    fn test_no_double_sign_same_block() {
        let kp = Keypair::generate();
        let validator = Address::from_pubkey(&kp.public);
        let block_hash = H256::from_slice(&[1u8; 32]);

        let vote1 = Vote {
            slot: 100,
            block_hash: block_hash.clone(),
            validator: validator.clone(),
            signature: sign(&kp.secret, b"vote1"),
        };

        let vote2 = Vote {
            slot: 100,
            block_hash: block_hash,
            validator: validator,
            signature: sign(&kp.secret, b"vote2"),
        };

        let proof = detect_double_sign(&vote1, &vote2);
        assert!(proof.is_none());
    }

    #[test]
    fn test_calculate_slash_amount() {
        let stake = 1_000_000u128;

        let double_sign_slash =
            calculate_slash_amount(stake, &SlashType::DoubleSign);
        assert_eq!(double_sign_slash, 50_000);

        let downtime_slash = calculate_slash_amount(
            stake,
            &SlashType::Downtime { missing_slots: 200 },
        );
        assert_eq!(downtime_slash, 200);
    }
}

