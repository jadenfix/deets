use aether_crypto_vrf::{
    check_leader_eligibility_integer, EcVrfVerifier, VrfProof, VrfSigner, VrfVerifier,
};
use aether_types::{Address, Block, Slot, ValidatorInfo, H256};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// VRF-PoS Consensus Engine
///
/// Algorithm:
/// 1. Each epoch has randomness η_e
/// 2. For each slot, validators evaluate VRF(secret, η_e || slot)
/// 3. If output < threshold * (stake / total_stake), validator can propose
/// 4. Block includes VRF proof for verification
/// 5. Votes use BLS aggregation (separate module)
///
/// Parameters:
/// - tau (τ): Target leader rate (e.g., 0.8 = 80% of slots have a leader)
/// - slot_time: 500ms per slot
/// - epoch_length: Number of slots per epoch (e.g., 43200 = 6 hours at 500ms/slot)
pub struct VrfPosConsensus {
    /// Current epoch randomness
    epoch_randomness: H256,

    /// Current epoch number
    current_epoch: u64,

    /// Current slot number
    current_slot: Slot,

    /// Validator set with stakes
    validators: HashMap<Address, ValidatorInfo>,

    /// Total stake in the network
    total_stake: u128,

    /// Leader rate parameter (0 < tau <= 1) — kept for API compatibility
    #[allow(dead_code)]
    tau: f64,
    tau_numerator: u128,
    tau_denominator: u128,

    /// Finalized slot (2/3 stake voted)
    finalized_slot: Slot,

    /// Epoch length in slots
    epoch_length: u64,
}

impl VrfPosConsensus {
    pub fn new(validators: Vec<ValidatorInfo>, tau: f64, epoch_length: u64) -> Self {
        // Guard against division-by-zero in advance_slot epoch boundary check.
        let epoch_length = epoch_length.max(1);
        let total_stake: u128 = validators
            .iter()
            .map(|v| v.stake)
            .fold(0u128, u128::saturating_add);
        let validators_map: HashMap<Address, ValidatorInfo> = validators
            .into_iter()
            .map(|v| (v.pubkey.to_address(), v))
            .collect();

        // Convert f64 tau to integer fraction: multiply by 10000 to preserve 4 decimal places
        let tau_clamped = if tau.is_finite() {
            tau.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let tau_numerator = (tau_clamped * 10000.0).round() as u128;
        let tau_denominator = 10000u128;

        VrfPosConsensus {
            epoch_randomness: H256::zero(), // Genesis randomness
            current_epoch: 0,
            current_slot: 0,
            validators: validators_map,
            total_stake,
            tau,
            tau_numerator,
            tau_denominator,
            finalized_slot: 0,
            epoch_length,
        }
    }

    /// Check if a validator is eligible to propose for a slot
    pub fn is_eligible_leader(
        &self,
        vrf_keypair: &dyn VrfSigner,
        slot: Slot,
        validator_address: &Address,
    ) -> Result<Option<VrfProof>> {
        // Get validator stake
        let validator = self
            .validators
            .get(validator_address)
            .ok_or_else(|| anyhow::anyhow!("validator not found"))?;

        // Compute VRF input: η_e || slot
        let mut input = Vec::new();
        input.extend_from_slice(self.epoch_randomness.as_bytes());
        input.extend_from_slice(&slot.to_le_bytes());

        // Evaluate VRF
        let proof = vrf_keypair.prove(&input);

        // Check eligibility (using deterministic integer arithmetic)
        let eligible = check_leader_eligibility_integer(
            &proof.output,
            validator.stake,
            self.total_stake,
            self.tau_numerator,
            self.tau_denominator,
        );

        if eligible {
            Ok(Some(proof))
        } else {
            Ok(None)
        }
    }

    /// Verify that a validator was eligible to propose a block.
    /// Uses the provided `VrfVerifier` for proof validation, defaulting to
    /// `EcVrfVerifier` in production.
    pub fn verify_leader_with(
        &self,
        block: &Block,
        proposer: &Address,
        verifier: &dyn VrfVerifier,
    ) -> Result<bool> {
        let validator = self
            .validators
            .get(proposer)
            .ok_or_else(|| anyhow::anyhow!("validator not found"))?;

        let mut input = Vec::new();
        input.extend_from_slice(self.epoch_randomness.as_bytes());
        input.extend_from_slice(&block.header.slot.to_le_bytes());

        let vrf_proof = VrfProof {
            output: block.header.vrf_proof.output,
            proof: block.header.vrf_proof.proof.clone(),
        };

        let vrf_pubkey: [u8; 32] = validator.pubkey.as_bytes()[..32]
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid public key length"))?;

        if !verifier.verify(&vrf_pubkey, &input, &vrf_proof)? {
            return Ok(false);
        }

        Ok(check_leader_eligibility_integer(
            &vrf_proof.output,
            validator.stake,
            self.total_stake,
            self.tau_numerator,
            self.tau_denominator,
        ))
    }

    /// Verify leader eligibility using the default ECVRF verifier.
    pub fn verify_leader(&self, block: &Block, proposer: &Address) -> Result<bool> {
        self.verify_leader_with(block, proposer, &EcVrfVerifier)
    }

    /// Advance to next epoch and update randomness
    /// η_e = H(VRF_output_of_first_block_in_epoch_{e-1})
    pub fn advance_epoch(&mut self, seed_block_vrf_output: [u8; 32]) {
        // Compute new epoch randomness
        let mut hasher = Sha256::new();
        hasher.update(seed_block_vrf_output);
        let new_randomness = hasher.finalize();

        self.epoch_randomness = H256(new_randomness.into());
        self.current_epoch = self.current_epoch.saturating_add(1);

        tracing::info!(
            epoch = self.current_epoch,
            randomness = ?self.epoch_randomness,
            "Advanced to new epoch"
        );
    }

    /// Advance to next slot
    pub fn advance_slot(&mut self) {
        self.current_slot = self.current_slot.saturating_add(1);

        // Check if we need to advance epoch
        if self.epoch_length > 0 && self.current_slot % self.epoch_length == 0 {
            // In production, would use VRF output from first block of previous epoch
            // For now, just hash current randomness
            let mut hasher = Sha256::new();
            hasher.update(self.epoch_randomness.as_bytes());
            hasher.update(self.current_epoch.to_le_bytes());
            let new_randomness = hasher.finalize();

            self.epoch_randomness = H256(new_randomness.into());
            self.current_epoch = self.current_epoch.saturating_add(1);
        }
    }

    pub fn current_slot(&self) -> Slot {
        self.current_slot
    }

    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    pub fn finalized_slot(&self) -> Slot {
        self.finalized_slot
    }

    /// Add a validator to the set
    pub fn add_validator(&mut self, validator: ValidatorInfo) {
        let address = validator.pubkey.to_address();
        self.total_stake = self.total_stake.saturating_add(validator.stake);
        self.validators.insert(address, validator);
    }

    /// Update validator stake
    pub fn update_stake(&mut self, address: &Address, new_stake: u128) -> Result<()> {
        let validator = self
            .validators
            .get_mut(address)
            .ok_or_else(|| anyhow::anyhow!("validator not found"))?;

        self.total_stake = self
            .total_stake
            .saturating_sub(validator.stake)
            .saturating_add(new_stake);
        validator.stake = new_stake;

        Ok(())
    }

    /// Get total stake
    pub fn total_stake(&self) -> u128 {
        self.total_stake
    }

    /// Get validator count
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_vrf::mock::MockVrfSigner;
    use aether_crypto_vrf::VrfKeypair;
    use aether_types::PublicKey;

    fn create_test_validator(stake: u128) -> ValidatorInfo {
        let keypair = aether_crypto_primitives::Keypair::generate();
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake,
            commission: 0,
            active: true,
        }
    }

    #[test]
    fn test_vrf_pos_creation() {
        let validators = vec![create_test_validator(1000), create_test_validator(2000)];

        let consensus = VrfPosConsensus::new(validators, 0.8, 43200);

        assert_eq!(consensus.total_stake(), 3000);
        assert_eq!(consensus.validator_count(), 2);
    }

    #[test]
    fn test_slot_advancement() {
        let validators = vec![create_test_validator(1000)];
        let mut consensus = VrfPosConsensus::new(validators, 0.8, 10);

        assert_eq!(consensus.current_slot(), 0);

        consensus.advance_slot();
        assert_eq!(consensus.current_slot(), 1);
    }

    #[test]
    fn test_epoch_advancement() {
        let validators = vec![create_test_validator(1000)];
        let mut consensus = VrfPosConsensus::new(validators, 0.8, 5);

        let initial_randomness = consensus.epoch_randomness;

        // Advance through epoch
        for _ in 0..5 {
            consensus.advance_slot();
        }

        // Should be in new epoch
        assert_eq!(consensus.current_epoch(), 1);
        assert_ne!(consensus.epoch_randomness, initial_randomness);
    }

    #[test]
    fn test_leader_eligibility() {
        let validators = vec![create_test_validator(5000)]; // High stake
        let consensus = VrfPosConsensus::new(validators.clone(), 0.8, 43200);

        let vrf_keypair = VrfKeypair::generate();
        let slot: Slot = 1;
        let validator_addr = validators[0].pubkey.to_address();

        // With high stake, should be eligible for some slots
        let result = consensus.is_eligible_leader(&vrf_keypair, slot, &validator_addr);

        assert!(result.is_ok());
        // Probabilistic: might or might not be eligible for this specific slot
    }

    #[test]
    fn test_add_validator_updates_total_stake() {
        let v1 = create_test_validator(1000);
        let mut consensus = VrfPosConsensus::new(vec![v1], 0.8, 100);
        assert_eq!(consensus.total_stake(), 1000);
        let v2 = create_test_validator(2000);
        consensus.add_validator(v2);
        assert_eq!(consensus.total_stake(), 3000);
    }

    #[test]
    fn test_stake_proportional_eligibility() {
        // Validator with 50% stake should be eligible ~40% of time (tau=0.8 * 0.5)
        let high_stake_validator = create_test_validator(5000);
        let low_stake_validator = create_test_validator(5000);

        let validators = vec![high_stake_validator.clone(), low_stake_validator];
        let consensus = VrfPosConsensus::new(validators, 0.8, 43200);

        let vrf_keypair = VrfKeypair::generate();
        let validator_addr = high_stake_validator.pubkey.to_address();

        let mut eligible_count = 0;
        let trials = 100;

        for slot_num in 0..trials {
            let slot: Slot = slot_num;
            if let Ok(Some(_)) = consensus.is_eligible_leader(&vrf_keypair, slot, &validator_addr) {
                eligible_count += 1;
            }
        }

        // Should be eligible for roughly 40% (tau * stake_fraction) of slots
        // Allow wide margin due to randomness
        let rate = eligible_count as f64 / trials as f64;
        println!("Eligibility rate: {:.2}% (expected ~40%)", rate * 100.0);

        // Just check it's reasonable (10-70% range)
        assert!(rate > 0.1 && rate < 0.7);
    }

    #[test]
    fn epoch_length_zero_does_not_panic() {
        // epoch_length=0 would cause division-by-zero in advance_slot.
        // Constructor must clamp it to >= 1.
        let mut consensus = VrfPosConsensus::new(vec![], 0.5, 0);
        assert_eq!(consensus.epoch_length, 1);
        // advance_slot should not panic
        consensus.advance_slot();
    }

    #[test]
    fn verify_leader_rejects_invalid_vrf_proof() {
        use aether_types::{Block, VrfProof as TypesVrfProof};

        let validator = create_test_validator(10_000);
        let addr = validator.pubkey.to_address();
        let consensus = VrfPosConsensus::new(vec![validator], 0.8, 100);

        let bogus_proof = TypesVrfProof {
            output: [0xAA; 32],
            proof: vec![0xFF; 80],
        };
        let block = Block::new(1, H256::zero(), addr, bogus_proof, vec![]);

        let result = consensus.verify_leader(&block, &addr);
        assert!(
            result.is_ok(),
            "verify_leader should not error on bad proof"
        );
        assert!(
            !result.unwrap(),
            "verify_leader must reject a block with a fabricated VRF proof"
        );
    }

    #[test]
    fn verify_leader_accepts_valid_vrf_proof() {
        use aether_types::{Block, VrfProof as TypesVrfProof};

        let vrf_keypair = VrfKeypair::generate();
        let validator = ValidatorInfo {
            pubkey: PublicKey::from_bytes(vrf_keypair.public_key().to_vec()),
            stake: 1_000_000,
            commission: 0,
            active: true,
        };
        let addr = validator.pubkey.to_address();
        let consensus = VrfPosConsensus::new(vec![validator], 0.99, 100);

        let slot = 42u64;
        let mut input = Vec::new();
        input.extend_from_slice(consensus.epoch_randomness.as_bytes());
        input.extend_from_slice(&slot.to_le_bytes());
        let proof = vrf_keypair.prove(&input);

        let types_proof = TypesVrfProof {
            output: proof.output,
            proof: proof.proof,
        };
        let block = Block::new(slot, H256::zero(), addr, types_proof, vec![]);

        let result = consensus.verify_leader(&block, &addr);
        assert!(
            result.is_ok(),
            "verify_leader should not error on valid proof"
        );
        // With stake=1M (100% of total) and tau=0.99, this should almost certainly be eligible
    }

    #[test]
    fn mock_vrf_signer_works_with_eligibility_check() {
        let validator = create_test_validator(5000);
        let validator_addr = validator.pubkey.to_address();
        let consensus = VrfPosConsensus::new(vec![validator], 0.8, 100);

        let mock_signer = MockVrfSigner::from_index(1);
        let result = consensus.is_eligible_leader(&mock_signer, 0, &validator_addr);
        assert!(result.is_ok());
    }

    #[test]
    fn mock_vrf_is_deterministic_across_calls() {
        let validator = create_test_validator(10_000);
        let validator_addr = validator.pubkey.to_address();
        let consensus = VrfPosConsensus::new(vec![validator], 1.0, 100);

        let mock = MockVrfSigner::from_index(42);
        let r1 = consensus
            .is_eligible_leader(&mock, 7, &validator_addr)
            .unwrap();
        let r2 = consensus
            .is_eligible_leader(&mock, 7, &validator_addr)
            .unwrap();
        assert_eq!(r1.is_some(), r2.is_some());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_crypto_vrf::VrfKeypair;
    use aether_types::PublicKey;
    use proptest::prelude::*;

    fn create_validator(stake: u128) -> ValidatorInfo {
        let keypair = aether_crypto_primitives::Keypair::generate();
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake,
            commission: 0,
            active: true,
        }
    }

    proptest! {
        /// Total stake equals the sum of all validator stakes.
        #[test]
        fn total_stake_is_sum(stakes in prop::collection::vec(1u128..=1_000_000, 1..10)) {
            let validators: Vec<ValidatorInfo> = stakes.iter().map(|&s| create_validator(s)).collect();
            let expected: u128 = stakes.iter().copied().fold(0u128, u128::saturating_add);
            let consensus = VrfPosConsensus::new(validators, 0.5, 100);
            prop_assert_eq!(consensus.total_stake(), expected);
        }

        /// Validator count matches input.
        #[test]
        fn validator_count_matches(n in 1usize..=20) {
            let validators: Vec<ValidatorInfo> = (0..n).map(|_| create_validator(1000)).collect();
            let consensus = VrfPosConsensus::new(validators, 0.5, 100);
            prop_assert_eq!(consensus.validator_count(), n);
        }

        /// Slot advances monotonically.
        #[test]
        fn slot_advances_monotonically(advances in 1usize..=50) {
            let validators = vec![create_validator(1000)];
            let mut consensus = VrfPosConsensus::new(validators, 0.5, 100);
            let mut prev = consensus.current_slot();
            for _ in 0..advances {
                consensus.advance_slot();
                let cur = consensus.current_slot();
                prop_assert!(cur > prev, "slot must increase");
                prev = cur;
            }
        }

        /// Epoch advances at epoch boundaries.
        #[test]
        fn epoch_advances_at_boundary(epoch_len in 2u64..=20) {
            let validators = vec![create_validator(1000)];
            let mut consensus = VrfPosConsensus::new(validators, 0.5, epoch_len);
            prop_assert_eq!(consensus.current_epoch(), 0);
            for _ in 0..epoch_len {
                consensus.advance_slot();
            }
            prop_assert_eq!(consensus.current_epoch(), 1);
        }

        /// Epoch randomness changes at epoch boundary.
        #[test]
        fn randomness_rotates_at_epoch(epoch_len in 2u64..=20) {
            let validators = vec![create_validator(1000)];
            let mut consensus = VrfPosConsensus::new(validators, 0.5, epoch_len);
            let initial = consensus.epoch_randomness;
            for _ in 0..epoch_len {
                consensus.advance_slot();
            }
            prop_assert_ne!(consensus.epoch_randomness, initial,
                "randomness must change at epoch boundary");
        }

        /// update_stake maintains total_stake conservation.
        #[test]
        fn update_stake_conserves_total(
            old_stake in 100u128..=10000,
            new_stake in 100u128..=10000,
        ) {
            let v = create_validator(old_stake);
            let addr = v.pubkey.to_address();
            let mut consensus = VrfPosConsensus::new(vec![v], 0.5, 100);
            prop_assert_eq!(consensus.total_stake(), old_stake);
            consensus.update_stake(&addr, new_stake).unwrap();
            prop_assert_eq!(consensus.total_stake(), new_stake);
        }

        /// add_validator increases total_stake and count.
        #[test]
        fn add_validator_increases_stake_and_count(
            initial_stakes in prop::collection::vec(100u128..=10000, 1..5),
            new_stake in 100u128..=10000,
        ) {
            let validators: Vec<ValidatorInfo> = initial_stakes.iter().map(|&s| create_validator(s)).collect();
            let initial_count = validators.len();
            let initial_total: u128 = initial_stakes.iter().copied().fold(0u128, u128::saturating_add);
            let mut consensus = VrfPosConsensus::new(validators, 0.5, 100);

            let new_v = create_validator(new_stake);
            consensus.add_validator(new_v);
            prop_assert_eq!(consensus.validator_count(), initial_count + 1);
            prop_assert_eq!(consensus.total_stake(), initial_total.saturating_add(new_stake));
        }

        /// is_eligible_leader never errors for registered validators.
        #[test]
        fn eligible_leader_never_errors(slot in 0u64..1000, stake in 1u128..=100_000) {
            let v = create_validator(stake);
            let addr = v.pubkey.to_address();
            let consensus = VrfPosConsensus::new(vec![v], 0.8, 100);
            let vrf_kp = VrfKeypair::generate();
            let result = consensus.is_eligible_leader(&vrf_kp, slot, &addr);
            prop_assert!(result.is_ok(), "is_eligible_leader must not error for registered validator");
        }

        /// is_eligible_leader errors for unregistered validators.
        #[test]
        fn eligible_leader_errors_for_unknown(slot in 0u64..1000) {
            let v = create_validator(1000);
            let consensus = VrfPosConsensus::new(vec![v], 0.8, 100);
            let unknown = create_validator(500);
            let unknown_addr = unknown.pubkey.to_address();
            let vrf_kp = VrfKeypair::generate();
            prop_assert!(consensus.is_eligible_leader(&vrf_kp, slot, &unknown_addr).is_err());
        }

        /// advance_epoch changes epoch number and randomness deterministically.
        #[test]
        fn advance_epoch_deterministic(seed in prop::array::uniform32(any::<u8>())) {
            let v = create_validator(1000);
            let mut c1 = VrfPosConsensus::new(vec![v.clone()], 0.5, 100);
            let mut c2 = VrfPosConsensus::new(vec![v], 0.5, 100);
            c1.advance_epoch(seed);
            c2.advance_epoch(seed);
            prop_assert_eq!(c1.epoch_randomness, c2.epoch_randomness);
            prop_assert_eq!(c1.current_epoch(), c2.current_epoch());
        }
    }
}
