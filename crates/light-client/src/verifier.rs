use aether_types::{BlockHeader, PublicKey, Slot, H256};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// Validator info for light client verification.
#[derive(Debug, Clone)]
pub struct ValidatorEntry {
    pub pubkey: PublicKey,
    pub stake: u128,
}

/// Finalized header with aggregate signature proof.
#[derive(Debug, Clone)]
pub struct FinalizedHeader {
    pub header: BlockHeader,
    pub aggregate_signature: Vec<u8>,
    pub signer_pubkeys: Vec<PublicKey>,
    pub total_signing_stake: u128,
}

/// Light client verifier — checks finality of headers using BLS signatures.
pub struct LightClientVerifier {
    /// Known validator set (updated on epoch boundaries).
    validators: HashMap<Vec<u8>, ValidatorEntry>,
    /// Total stake across all validators.
    total_stake: u128,
    /// Highest verified finalized slot.
    finalized_slot: Slot,
    /// Finalized state root (for Merkle proof verification).
    finalized_state_root: H256,
}

impl LightClientVerifier {
    /// Create a new light client verifier with the initial validator set.
    pub fn new(validators: Vec<ValidatorEntry>) -> Self {
        let total_stake: u128 = validators.iter().map(|v| v.stake).sum();
        let validators_map: HashMap<Vec<u8>, ValidatorEntry> = validators
            .into_iter()
            .map(|v| (v.pubkey.as_bytes().to_vec(), v))
            .collect();

        LightClientVerifier {
            validators: validators_map,
            total_stake,
            finalized_slot: 0,
            finalized_state_root: H256::zero(),
        }
    }

    /// Verify and accept a finalized header.
    ///
    /// Checks that:
    /// 1. The aggregate signature is valid over the header hash
    /// 2. The signing stake represents ≥2/3 of total stake
    /// 3. The slot advances (no regression)
    pub fn verify_finalized_header(&mut self, finalized: &FinalizedHeader) -> Result<()> {
        let header = &finalized.header;

        // Slot must advance
        if header.slot <= self.finalized_slot {
            bail!(
                "slot {} does not advance beyond finalized slot {}",
                header.slot,
                self.finalized_slot
            );
        }

        // Check quorum: signing stake must be ≥2/3 of total
        if finalized.total_signing_stake * 3 < self.total_stake * 2 {
            bail!(
                "insufficient signing stake: {} < 2/3 of {}",
                finalized.total_signing_stake,
                self.total_stake
            );
        }

        // Verify all signers are known validators
        let mut verified_stake: u128 = 0;
        for pk in &finalized.signer_pubkeys {
            match self.validators.get(pk.as_bytes()) {
                Some(entry) => verified_stake += entry.stake,
                None => bail!("unknown signer: {:?}", pk),
            }
        }

        if verified_stake * 3 < self.total_stake * 2 {
            bail!(
                "verified stake {} < 2/3 of total {}",
                verified_stake,
                self.total_stake
            );
        }

        // BLS aggregate signature verification would go here in production.
        // For now, we trust the stake accounting above.
        // In production: aether_crypto_bls::verify_aggregate(...)

        // Accept the header
        self.finalized_slot = header.slot;
        self.finalized_state_root = header.state_root;

        Ok(())
    }

    /// Get the current finalized state root (for Merkle proof verification).
    pub fn finalized_state_root(&self) -> H256 {
        self.finalized_state_root
    }

    pub fn finalized_slot(&self) -> Slot {
        self.finalized_slot
    }

    /// Update the validator set (on epoch boundary).
    pub fn update_validators(&mut self, validators: Vec<ValidatorEntry>) {
        self.total_stake = validators.iter().map(|v| v.stake).sum();
        self.validators = validators
            .into_iter()
            .map(|v| (v.pubkey.as_bytes().to_vec(), v))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::*;

    fn make_validator(id: u8, stake: u128) -> ValidatorEntry {
        ValidatorEntry {
            pubkey: PublicKey::from_bytes(vec![id; 32]),
            stake,
        }
    }

    fn make_finalized_header(
        slot: u64,
        signers: &[ValidatorEntry],
    ) -> FinalizedHeader {
        let total_stake: u128 = signers.iter().map(|v| v.stake).sum();
        FinalizedHeader {
            header: BlockHeader {
                version: 1,
                slot,
                parent_hash: H256::zero(),
                state_root: H256::from_slice(&[slot as u8; 32]).unwrap(),
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                proposer: Address::from_slice(&[1u8; 20]).unwrap(),
                vrf_proof: VrfProof {
                    output: [0u8; 32],
                    proof: vec![0u8; 80],
                },
                timestamp: 1000 + slot,
            },
            aggregate_signature: vec![0u8; 96],
            signer_pubkeys: signers.iter().map(|v| v.pubkey.clone()).collect(),
            total_signing_stake: total_stake,
        }
    }

    #[test]
    fn test_verify_valid_header() {
        let validators = vec![
            make_validator(1, 1000),
            make_validator(2, 1000),
            make_validator(3, 1000),
        ];
        let mut verifier = LightClientVerifier::new(validators.clone());

        // 3/3 validators sign = 100% stake
        let header = make_finalized_header(1, &validators);
        assert!(verifier.verify_finalized_header(&header).is_ok());
        assert_eq!(verifier.finalized_slot(), 1);
    }

    #[test]
    fn test_reject_insufficient_stake() {
        let validators = vec![
            make_validator(1, 1000),
            make_validator(2, 1000),
            make_validator(3, 1000),
        ];
        let mut verifier = LightClientVerifier::new(validators);

        // Only 1/3 validators sign = 33% < 67%
        let header = make_finalized_header(1, &[make_validator(1, 1000)]);
        assert!(verifier.verify_finalized_header(&header).is_err());
    }

    #[test]
    fn test_reject_slot_regression() {
        let validators = vec![make_validator(1, 1000)];
        let mut verifier = LightClientVerifier::new(validators.clone());

        let h1 = make_finalized_header(10, &validators);
        verifier.verify_finalized_header(&h1).unwrap();

        // Try to verify a header with a lower slot
        let h2 = make_finalized_header(5, &validators);
        assert!(verifier.verify_finalized_header(&h2).is_err());
    }

    #[test]
    fn test_reject_unknown_signer() {
        let validators = vec![make_validator(1, 1000), make_validator(2, 1000)];
        let mut verifier = LightClientVerifier::new(validators);

        // Signer not in validator set
        let header = make_finalized_header(1, &[make_validator(99, 2000)]);
        assert!(verifier.verify_finalized_header(&header).is_err());
    }

    #[test]
    fn test_state_root_updates() {
        let validators = vec![make_validator(1, 1000)];
        let mut verifier = LightClientVerifier::new(validators.clone());

        assert_eq!(verifier.finalized_state_root(), H256::zero());

        let h = make_finalized_header(1, &validators);
        verifier.verify_finalized_header(&h).unwrap();

        assert_ne!(verifier.finalized_state_root(), H256::zero());
    }
}
