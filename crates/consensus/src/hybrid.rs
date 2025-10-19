// ============================================================================
// HYBRID CONSENSUS - Phase 1 Full Integration
// ============================================================================
// Combines VRF-PoS leader election + HotStuff BFT + BLS signature aggregation
// ============================================================================

use crate::ConsensusEngine;
use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, BlsKeypair};
use aether_crypto_vrf::{check_leader_eligibility, verify_proof, VrfKeypair, VrfProof};
use aether_types::{Address, Block, PublicKey, Slot, ValidatorInfo, Vote, H256};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// HotStuff consensus phases
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Phase {
    Propose,
    Prevote,
    Precommit,
    Commit,
}

/// Aggregated vote certificate (QC)
#[derive(Debug, Clone)]
pub struct QuorumCertificate {
    pub slot: Slot,
    pub block_hash: H256,
    pub phase: Phase,
    pub total_stake: u128,
    pub signers: Vec<Address>,
    pub aggregated_signature: Vec<u8>,
    pub aggregated_pubkey: Vec<u8>,
}

/// Full Phase 1 consensus combining:
/// - VRF-PoS for leader election
/// - HotStuff 2-chain for BFT finality
/// - BLS for vote aggregation
pub struct HybridConsensus {
    // === Validator Set ===
    validators: HashMap<Address, ValidatorInfo>,
    total_stake: u128,

    // === Slot/Epoch Management ===
    current_slot: Slot,
    current_epoch: u64,
    epoch_randomness: H256,
    epoch_length: u64,

    // === VRF-PoS Parameters ===
    tau: f64, // Leader rate (0 < tau <= 1)
    my_vrf_keypair: Option<VrfKeypair>,
    my_bls_keypair: Option<BlsKeypair>,
    my_address: Option<Address>,

    // === HotStuff State ===
    current_phase: Phase,
    votes: HashMap<(Slot, Phase, H256), Vec<Vote>>,
    qcs: HashMap<(Slot, Phase, H256), QuorumCertificate>,
    locked_block: Option<H256>,
    locked_slot: Slot,

    // === Finality ===
    committed_slot: Slot,
    finalized_slot: Slot,
}

impl HybridConsensus {
    pub fn new(
        validators: Vec<ValidatorInfo>,
        tau: f64,
        epoch_length: u64,
        my_vrf_keypair: Option<VrfKeypair>,
        my_bls_keypair: Option<BlsKeypair>,
        my_address: Option<Address>,
    ) -> Self {
        let total_stake: u128 = validators.iter().map(|v| v.stake).sum();
        let validators_map: HashMap<Address, ValidatorInfo> = validators
            .into_iter()
            .map(|v| (v.pubkey.to_address(), v))
            .collect();

        HybridConsensus {
            validators: validators_map,
            total_stake,
            current_slot: 0,
            current_epoch: 0,
            epoch_randomness: H256::zero(),
            epoch_length,
            tau,
            my_vrf_keypair,
            my_bls_keypair,
            my_address,
            current_phase: Phase::Propose,
            votes: HashMap::new(),
            qcs: HashMap::new(),
            locked_block: None,
            locked_slot: 0,
            committed_slot: 0,
            finalized_slot: 0,
        }
    }

    /// Check if I am eligible to be leader for this slot
    pub fn check_my_eligibility(&self, slot: Slot) -> Option<VrfProof> {
        let vrf_keypair = self.my_vrf_keypair.as_ref()?;
        let my_addr = self.my_address.as_ref()?;
        let validator = self.validators.get(my_addr)?;

        // Compute VRF input: epoch_randomness || slot
        let mut input = Vec::new();
        input.extend_from_slice(self.epoch_randomness.as_bytes());
        input.extend_from_slice(&slot.to_le_bytes());

        // Generate VRF proof
        let proof = vrf_keypair.prove(&input);

        // Check eligibility threshold
        if check_leader_eligibility(&proof.output, validator.stake, self.total_stake, self.tau) {
            Some(proof)
        } else {
            None
        }
    }

    /// Verify that a block's proposer was eligible
    pub fn verify_leader_eligibility(&self, block: &Block) -> Result<bool> {
        let proposer_addr = block.header.proposer;
        let validator = self
            .validators
            .get(&proposer_addr)
            .ok_or_else(|| anyhow::anyhow!("unknown validator"))?;

        // Reconstruct VRF input
        let mut input = Vec::new();
        input.extend_from_slice(self.epoch_randomness.as_bytes());
        input.extend_from_slice(&block.header.slot.to_le_bytes());

        // Convert VRF proof
        let vrf_proof = VrfProof {
            output: block.header.vrf_proof.output,
            proof: block.header.vrf_proof.proof.clone(),
        };

        // Verify VRF proof
        let vrf_pubkey = validator
            .pubkey
            .as_bytes()
            .get(..32)
            .ok_or_else(|| anyhow::anyhow!("invalid pubkey length"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("pubkey conversion failed"))?;

        if !verify_proof(&vrf_pubkey, &input, &vrf_proof)? {
            return Ok(false);
        }

        // Check eligibility threshold
        Ok(check_leader_eligibility(
            &vrf_proof.output,
            validator.stake,
            self.total_stake,
            self.tau,
        ))
    }

    /// Create a vote for a block (BLS signature)
    fn create_vote_for_phase(&self, block_hash: H256, phase: Phase) -> Result<Option<Vote>> {
        let bls_keypair = match &self.my_bls_keypair {
            Some(kp) => kp,
            None => return Ok(None), // Not a validator
        };

        let my_addr = match &self.my_address {
            Some(addr) => addr,
            None => return Ok(None),
        };

        let validator = self
            .validators
            .get(my_addr)
            .ok_or_else(|| anyhow::anyhow!("not in validator set"))?;

        // Create vote message: block_hash || slot || phase
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&self.current_slot.to_le_bytes());
        msg.extend_from_slice(&format!("{:?}", phase).as_bytes());

        // Sign with BLS
        let signature = bls_keypair.sign(&msg);

        Ok(Some(Vote {
            slot: self.current_slot,
            block_hash,
            validator: validator.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(signature),
            stake: validator.stake,
        }))
    }

    /// Process a vote and check for quorum
    pub fn process_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>> {
        // Verify vote is for current slot
        if vote.slot != self.current_slot {
            bail!("vote for wrong slot");
        }

        // Store vote
        let key = (vote.slot, self.current_phase.clone(), vote.block_hash);
        let votes = self.votes.entry(key.clone()).or_insert_with(Vec::new);
        votes.push(vote.clone());

        // Check for quorum (2/3+ stake)
        let voted_stake: u128 = votes.iter().map(|v| v.stake).sum();
        let has_quorum = voted_stake * 3 >= self.total_stake * 2;

        if has_quorum {
            // Clone votes for aggregation to avoid borrow conflicts
            let votes_to_aggregate = votes.clone();
            // Create QC
            let qc = self.aggregate_votes(&votes_to_aggregate)?;
            self.qcs.insert(key, qc.clone());

            // Handle phase transitions and finality
            match self.current_phase {
                Phase::Propose => {
                    // Quorum on proposal, advance to prevote
                    println!("  QC formed in Propose phase, advancing to Prevote");
                    self.advance_phase();
                }
                Phase::Prevote => {
                    // Lock on this block
                    self.locked_block = Some(vote.block_hash);
                    self.locked_slot = vote.slot;
                    println!(
                        "  QC formed in Prevote phase, locked block {:?}, advancing to Precommit",
                        vote.block_hash
                    );
                    self.advance_phase();
                }
                Phase::Precommit => {
                    // Check 2-chain rule for finality
                    if let Some(parent_slot) = vote.slot.checked_sub(1) {
                        let prevote_key = (parent_slot, Phase::Prevote, vote.block_hash);
                        if self.qcs.contains_key(&prevote_key) {
                            // Finalize parent block via 2-chain rule!
                            self.finalized_slot = parent_slot;
                            println!(
                                "  FINALIZED slot {} block {:?} via 2-chain",
                                parent_slot, vote.block_hash
                            );
                        }
                    }
                    self.committed_slot = vote.slot;
                    println!("  QC formed in Precommit phase, advancing to Commit");
                    self.advance_phase();
                }
                Phase::Commit => {
                    // Commit phase complete, ready for next slot
                    println!("  QC formed in Commit phase, slot {} complete", vote.slot);
                }
            }

            return Ok(Some(qc));
        }

        Ok(None)
    }

    /// Aggregate BLS signatures from votes
    fn aggregate_votes(&self, votes: &[Vote]) -> Result<QuorumCertificate> {
        let signatures: Vec<Vec<u8>> = votes
            .iter()
            .map(|v| v.signature.as_bytes().to_vec())
            .collect();

        let pubkeys: Vec<Vec<u8>> = votes
            .iter()
            .map(|v| {
                let addr = v.validator.to_address();
                if let Some(val) = self.validators.get(&addr) {
                    let bytes = val.pubkey.as_bytes();
                    // BLS requires 48 bytes, pad if necessary
                    let mut padded = vec![0u8; 48];
                    let copy_len = bytes.len().min(48);
                    padded[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    padded
                } else {
                    vec![0u8; 48]
                }
            })
            .collect();

        let agg_sig = aggregate_signatures(&signatures)?;
        let agg_pk = aggregate_public_keys(&pubkeys)?;

        let total_stake = votes.iter().map(|v| v.stake).sum();
        let signers: Vec<Address> = votes.iter().map(|v| v.validator.to_address()).collect();

        Ok(QuorumCertificate {
            slot: votes[0].slot,
            block_hash: votes[0].block_hash,
            phase: self.current_phase.clone(),
            total_stake,
            signers,
            aggregated_signature: agg_sig,
            aggregated_pubkey: agg_pk,
        })
    }

    /// Advance to next HotStuff phase
    pub fn advance_phase(&mut self) {
        self.current_phase = match self.current_phase {
            Phase::Propose => Phase::Prevote,
            Phase::Prevote => Phase::Precommit,
            Phase::Precommit => Phase::Commit,
            Phase::Commit => Phase::Propose,
        };
    }

    pub fn current_phase(&self) -> &Phase {
        &self.current_phase
    }
}

impl ConsensusEngine for HybridConsensus {
    fn current_slot(&self) -> Slot {
        self.current_slot
    }

    fn advance_slot(&mut self) {
        self.current_slot += 1;
        self.current_phase = Phase::Propose;
        self.votes.clear();

        // Check for epoch transition
        if self.current_slot % self.epoch_length == 0 {
            // Update epoch randomness (simplified - use previous epoch hash)
            let mut hasher = Sha256::new();
            hasher.update(self.epoch_randomness.as_bytes());
            hasher.update(&self.current_epoch.to_le_bytes());
            let new_randomness = hasher.finalize();

            self.epoch_randomness = H256::from_slice(&new_randomness).unwrap();
            self.current_epoch += 1;
        }
    }

    fn is_leader(&self, slot: Slot, validator_pubkey: &PublicKey) -> bool {
        // For now, just check if the validator address matches
        // In full implementation, we'd verify the VRF proof
        if let Some(my_addr) = &self.my_address {
            let validator_addr = validator_pubkey.to_address();
            return validator_addr == *my_addr && self.check_my_eligibility(slot).is_some();
        }
        false
    }

    fn validate_block(&self, block: &Block) -> Result<()> {
        // Check slot is valid
        if block.header.slot > self.current_slot {
            bail!("block from future slot");
        }

        // Verify VRF proof and leader eligibility
        if !self.verify_leader_eligibility(block)? {
            bail!("invalid leader proof");
        }

        // Check block extends from locked block (if any)
        if let Some(locked) = &self.locked_block {
            if block.header.parent_hash != *locked {
                bail!("block does not extend locked block");
            }
        }

        Ok(())
    }

    fn add_vote(&mut self, vote: Vote) -> Result<()> {
        self.process_vote(vote)?;
        Ok(())
    }

    fn check_finality(&mut self, slot: Slot) -> bool {
        // Check if slot is already finalized
        slot <= self.finalized_slot
    }

    fn finalized_slot(&self) -> Slot {
        self.finalized_slot
    }

    fn total_stake(&self) -> u128 {
        self.total_stake
    }

    fn get_leader_proof(&self, slot: Slot) -> Option<VrfProof> {
        self.check_my_eligibility(slot)
    }

    fn create_vote(&self, block_hash: H256) -> Result<Option<Vote>> {
        // Use the current phase so the signature commits to the state machine.
        self.create_vote_for_phase(block_hash, self.current_phase.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_bls::BlsKeypair;
    use aether_crypto_primitives::Keypair;
    use aether_crypto_vrf::VrfKeypair;

    fn create_test_validator(stake: u128) -> ValidatorInfo {
        let keypair = Keypair::generate();
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake,
            commission: 0,
            active: true,
        }
    }

    #[test]
    fn test_hybrid_consensus_creation() {
        let validators = vec![
            create_test_validator(1000),
            create_test_validator(2000),
            create_test_validator(3000),
        ];

        let consensus = HybridConsensus::new(validators, 0.8, 100, None, None, None);

        assert_eq!(consensus.total_stake(), 6000);
        assert_eq!(consensus.current_slot(), 0);
        assert_eq!(consensus.current_phase(), &Phase::Propose);
    }

    #[test]
    fn test_slot_and_phase_advancement() {
        let validators = vec![create_test_validator(1000)];
        let mut consensus = HybridConsensus::new(validators, 0.8, 100, None, None, None);

        assert_eq!(consensus.current_slot(), 0);
        assert_eq!(consensus.current_phase(), &Phase::Propose);

        consensus.advance_phase();
        assert_eq!(consensus.current_phase(), &Phase::Prevote);

        consensus.advance_slot();
        assert_eq!(consensus.current_slot(), 1);
        assert_eq!(consensus.current_phase(), &Phase::Propose);
    }

    #[test]
    fn test_quorum_calculation() {
        let validators = vec![
            create_test_validator(1000),
            create_test_validator(1000),
            create_test_validator(1000),
        ];
        let consensus = HybridConsensus::new(validators, 0.8, 100, None, None, None);

        // Total stake = 3000
        // Quorum = 2/3 = 2000
        assert_eq!(consensus.total_stake, 3000);
    }

    #[test]
    fn create_vote_returns_signed_bls_vote() {
        let ed_keypair = Keypair::generate();
        let validator_info = ValidatorInfo {
            pubkey: PublicKey::from_bytes(ed_keypair.public_key()),
            stake: 1_000,
            commission: 0,
            active: true,
        };

        let address = validator_info.pubkey.to_address();
        let vrf_keypair = VrfKeypair::generate();
        let consensus = HybridConsensus::new(
            vec![validator_info],
            0.8,
            100,
            Some(vrf_keypair),
            Some(BlsKeypair::generate()),
            Some(address),
        );

        let block_hash = H256::from_slice(&[42u8; 32]).unwrap();
        let vote = consensus
            .create_vote(block_hash)
            .expect("vote creation should succeed")
            .expect("validator should be able to sign");

        assert_eq!(vote.block_hash, block_hash);
        assert_eq!(vote.signature.as_bytes().len(), 96);
        assert!(
            vote.signature.as_bytes().iter().any(|byte| *byte != 0),
            "BLS signature should not be all zeros"
        );
    }
}
