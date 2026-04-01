use aether_types::{BlockHeader, PublicKey, Slot, H256};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
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

/// Check if `voted_stake` represents a 2/3 quorum of `total_stake`.
/// Uses checked arithmetic to avoid overflow.
fn has_quorum(voted_stake: u128, total_stake: u128) -> bool {
    if total_stake == 0 {
        return false;
    }
    match (voted_stake.checked_mul(3), total_stake.checked_mul(2)) {
        (Some(lhs), Some(rhs)) => lhs >= rhs,
        _ => {
            let threshold = total_stake / 3 * 2 + if total_stake % 3 > 0 { 1 } else { 0 };
            voted_stake >= threshold
        }
    }
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
        if !has_quorum(finalized.total_signing_stake, self.total_stake) {
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

        if !has_quorum(verified_stake, self.total_stake) {
            bail!(
                "verified stake {} < 2/3 of total {}",
                verified_stake,
                self.total_stake
            );
        }

        // BLS aggregate signature verification:
        // 1. Aggregate the individual signer public keys
        // 2. Verify the aggregate signature over the header hash
        let signer_pk_bytes: Vec<Vec<u8>> = finalized
            .signer_pubkeys
            .iter()
            .map(|pk| pk.as_bytes().to_vec())
            .collect();

        // Compute the header hash that was signed
        let mut hasher = Sha256::new();
        hasher.update(header.slot.to_le_bytes());
        hasher.update(header.parent_hash.as_bytes());
        hasher.update(header.state_root.as_bytes());
        hasher.update(header.transactions_root.as_bytes());
        hasher.update(header.receipts_root.as_bytes());
        let header_msg = hasher.finalize().to_vec();

        // Aggregate public keys and verify
        if !finalized.aggregate_signature.is_empty() && !signer_pk_bytes.is_empty() {
            let agg_pk = aether_crypto_bls::aggregate_public_keys(&signer_pk_bytes)
                .map_err(|e| anyhow::anyhow!("failed to aggregate signer public keys: {e}"))?;

            let valid = aether_crypto_bls::verify_aggregated(
                &agg_pk,
                &header_msg,
                &finalized.aggregate_signature,
            )
            .map_err(|e| anyhow::anyhow!("BLS verification error: {e}"))?;

            if !valid {
                bail!(
                    "invalid BLS aggregate signature on finalized header at slot {}",
                    header.slot
                );
            }
        } else if finalized.aggregate_signature.is_empty() {
            bail!(
                "finalized header at slot {} has empty aggregate signature",
                header.slot
            );
        }

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
    use aether_crypto_bls::BlsKeypair;
    use aether_types::*;

    struct TestValidator {
        bls_kp: BlsKeypair,
        entry: ValidatorEntry,
    }

    fn make_test_validator(stake: u128) -> TestValidator {
        let bls_kp = BlsKeypair::generate();
        let entry = ValidatorEntry {
            pubkey: PublicKey::from_bytes(bls_kp.public_key()),
            stake,
        };
        TestValidator { bls_kp, entry }
    }

    /// Compute the header message that signers sign (must match verifier).
    fn header_message(header: &BlockHeader) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(header.slot.to_le_bytes());
        hasher.update(header.parent_hash.as_bytes());
        hasher.update(header.state_root.as_bytes());
        hasher.update(header.transactions_root.as_bytes());
        hasher.update(header.receipts_root.as_bytes());
        hasher.finalize().to_vec()
    }

    fn make_finalized_header(slot: u64, test_validators: &[&TestValidator]) -> FinalizedHeader {
        let header = BlockHeader {
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
        };

        let msg = header_message(&header);

        // Each validator signs, then aggregate
        let signatures: Vec<Vec<u8>> = test_validators
            .iter()
            .map(|tv| tv.bls_kp.sign(&msg))
            .collect();

        let agg_sig = aether_crypto_bls::aggregate_signatures(&signatures).unwrap();
        let total_stake: u128 = test_validators.iter().map(|tv| tv.entry.stake).sum();

        FinalizedHeader {
            header,
            aggregate_signature: agg_sig,
            signer_pubkeys: test_validators
                .iter()
                .map(|tv| tv.entry.pubkey.clone())
                .collect(),
            total_signing_stake: total_stake,
        }
    }

    #[test]
    fn test_verify_valid_header() {
        let tvs: Vec<TestValidator> = (0..3).map(|_| make_test_validator(1000)).collect();
        let entries: Vec<ValidatorEntry> = tvs.iter().map(|tv| tv.entry.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries);

        let refs: Vec<&TestValidator> = tvs.iter().collect();
        let header = make_finalized_header(1, &refs);
        assert!(verifier.verify_finalized_header(&header).is_ok());
        assert_eq!(verifier.finalized_slot(), 1);
    }

    #[test]
    fn test_reject_insufficient_stake() {
        let tvs: Vec<TestValidator> = (0..3).map(|_| make_test_validator(1000)).collect();
        let entries: Vec<ValidatorEntry> = tvs.iter().map(|tv| tv.entry.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries);

        // Only 1/3 validators sign = 33% < 67%
        let header = make_finalized_header(1, &[&tvs[0]]);
        assert!(verifier.verify_finalized_header(&header).is_err());
    }

    #[test]
    fn test_reject_slot_regression() {
        let tvs = [make_test_validator(1000)];
        let entries: Vec<ValidatorEntry> = tvs.iter().map(|tv| tv.entry.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries);

        let refs: Vec<&TestValidator> = tvs.iter().collect();
        let h1 = make_finalized_header(10, &refs);
        verifier.verify_finalized_header(&h1).unwrap();

        let h2 = make_finalized_header(5, &refs);
        assert!(verifier.verify_finalized_header(&h2).is_err());
    }

    #[test]
    fn test_reject_unknown_signer() {
        let tvs: Vec<TestValidator> = (0..2).map(|_| make_test_validator(1000)).collect();
        let entries: Vec<ValidatorEntry> = tvs.iter().map(|tv| tv.entry.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries);

        // Signer not in validator set
        let unknown = make_test_validator(2000);
        let header = make_finalized_header(1, &[&unknown]);
        assert!(verifier.verify_finalized_header(&header).is_err());
    }

    #[test]
    fn test_reject_forged_signature() {
        let tvs: Vec<TestValidator> = (0..3).map(|_| make_test_validator(1000)).collect();
        let entries: Vec<ValidatorEntry> = tvs.iter().map(|tv| tv.entry.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries);

        let refs: Vec<&TestValidator> = tvs.iter().collect();
        let mut header = make_finalized_header(1, &refs);
        // Forge the signature
        header.aggregate_signature = vec![0u8; 96];
        assert!(
            verifier.verify_finalized_header(&header).is_err(),
            "forged BLS signature must be rejected"
        );
    }

    #[test]
    fn test_state_root_updates() {
        let tvs = [make_test_validator(1000)];
        let entries: Vec<ValidatorEntry> = tvs.iter().map(|tv| tv.entry.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries);

        assert_eq!(verifier.finalized_state_root(), H256::zero());

        let refs: Vec<&TestValidator> = tvs.iter().collect();
        let h = make_finalized_header(1, &refs);
        verifier.verify_finalized_header(&h).unwrap();

        assert_ne!(verifier.finalized_state_root(), H256::zero());
    }
}
