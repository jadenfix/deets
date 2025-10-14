use aether_crypto_vrf::{check_leader_eligibility, VrfKeypair, VrfProof};
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

    /// Leader rate parameter (0 < tau <= 1)
    tau: f64,

    /// Finalized slot (2/3 stake voted)
    finalized_slot: Slot,

    /// Epoch length in slots
    epoch_length: u64,
}

impl VrfPosConsensus {
    pub fn new(validators: Vec<ValidatorInfo>, tau: f64, epoch_length: u64) -> Self {
        let total_stake: u128 = validators.iter().map(|v| v.stake).sum();
        let validators_map: HashMap<Address, ValidatorInfo> = validators
            .into_iter()
            .map(|v| (v.pubkey.to_address(), v))
            .collect();

        VrfPosConsensus {
            epoch_randomness: H256::zero(), // Genesis randomness
            current_epoch: 0,
            current_slot: 0,
            validators: validators_map,
            total_stake,
            tau,
            finalized_slot: 0,
            epoch_length,
        }
    }

    /// Check if a validator is eligible to propose for a slot
    pub fn is_eligible_leader(
        &self,
        vrf_keypair: &VrfKeypair,
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

        // Check eligibility
        let eligible =
            check_leader_eligibility(&proof.output, validator.stake, self.total_stake, self.tau);

        if eligible {
            Ok(Some(proof))
        } else {
            Ok(None)
        }
    }

    /// Verify that a validator was eligible to propose a block
    pub fn verify_leader(&self, block: &Block, proposer: &Address) -> Result<bool> {
        // Get validator
        let validator = self
            .validators
            .get(proposer)
            .ok_or_else(|| anyhow::anyhow!("validator not found"))?;

        // Reconstruct VRF input
        let mut input = Vec::new();
        input.extend_from_slice(self.epoch_randomness.as_bytes());
        input.extend_from_slice(&block.header.slot.to_le_bytes());

        // Convert VRF proof from block
        let vrf_proof = VrfProof {
            output: block.header.vrf_proof.output,
            proof: block.header.vrf_proof.proof.clone(),
        };

        // Verify VRF proof
        let vrf_pubkey: [u8; 32] = validator.pubkey.as_bytes()[..32]
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid public key length"))?;
        aether_crypto_vrf::verify_proof(&vrf_pubkey, &input, &vrf_proof)?;

        // Check eligibility threshold
        let eligible = check_leader_eligibility(
            &vrf_proof.output,
            validator.stake,
            self.total_stake,
            self.tau,
        );

        Ok(eligible)
    }

    /// Advance to next epoch and update randomness
    /// η_e = H(VRF_output_of_first_block_in_epoch_{e-1})
    pub fn advance_epoch(&mut self, seed_block_vrf_output: [u8; 32]) {
        // Compute new epoch randomness
        let mut hasher = Sha256::new();
        hasher.update(&seed_block_vrf_output);
        let new_randomness = hasher.finalize();

        self.epoch_randomness = H256::from_slice(&new_randomness).unwrap();
        self.current_epoch += 1;

        println!(
            "Advanced to epoch {}, new randomness: {:?}",
            self.current_epoch, &self.epoch_randomness
        );
    }

    /// Advance to next slot
    pub fn advance_slot(&mut self) {
        self.current_slot += 1;

        // Check if we need to advance epoch
        if self.current_slot % self.epoch_length == 0 {
            // In production, would use VRF output from first block of previous epoch
            // For now, just hash current randomness
            let mut hasher = Sha256::new();
            hasher.update(self.epoch_randomness.as_bytes());
            hasher.update(&self.current_epoch.to_le_bytes());
            let new_randomness = hasher.finalize();

            self.epoch_randomness = H256::from_slice(&new_randomness).unwrap();
            self.current_epoch += 1;
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
        self.total_stake += validator.stake;
        self.validators.insert(address, validator);
    }

    /// Update validator stake
    pub fn update_stake(&mut self, address: &Address, new_stake: u128) -> Result<()> {
        let validator = self
            .validators
            .get_mut(address)
            .ok_or_else(|| anyhow::anyhow!("validator not found"))?;

        self.total_stake = self.total_stake - validator.stake + new_stake;
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
}
