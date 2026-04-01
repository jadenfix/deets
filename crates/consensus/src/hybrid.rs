// ============================================================================
// HYBRID CONSENSUS - Phase 1 Full Integration
// ============================================================================
// Combines VRF-PoS leader election + HotStuff BFT + BLS signature aggregation
// ============================================================================

use crate::{ConsensusEngine, Pacemaker};
use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, BlsKeypair};
use aether_crypto_vrf::{check_leader_eligibility_integer, verify_proof, VrfKeypair, VrfProof};
use aether_types::{Address, Block, PublicKey, Slot, ValidatorInfo, Vote, H256};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::Duration;

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
    #[allow(dead_code)]
    tau: f64, // Leader rate (0 < tau <= 1) — kept for API compatibility
    tau_numerator: u128, // Integer numerator for deterministic eligibility check
    tau_denominator: u128, // Integer denominator for deterministic eligibility check
    my_vrf_keypair: Option<VrfKeypair>,
    my_bls_keypair: Option<BlsKeypair>,
    my_address: Option<Address>,

    // === HotStuff State ===
    current_phase: Phase,
    /// Votes deduplicated by validator address: one vote per (slot, phase, block, validator).
    votes: HashMap<(Slot, Phase, H256), HashMap<Address, Vote>>,
    qcs: HashMap<(Slot, Phase, H256), QuorumCertificate>,
    locked_block: Option<H256>,
    locked_slot: Slot,

    // === Block Parent Tracking (for 2-chain finality) ===
    block_parents: HashMap<H256, H256>,
    block_slots: HashMap<H256, Slot>,

    /// Track which block each validator voted for per slot (equivocation detection).
    /// Key: (slot, validator_address) → block_hash they voted for.
    vote_record: HashMap<(Slot, Address), H256>,

    // === VRF Public Keys (for verifying other validators' proofs) ===
    vrf_pubkeys: HashMap<Address, [u8; 32]>,
    /// BLS public keys for vote signature verification
    bls_pubkeys: HashMap<Address, Vec<u8>>,

    // === Pacemaker (timeout-based phase advancement) ===
    pacemaker: Pacemaker,

    // === Finality ===
    committed_slot: Slot,
    finalized_slot: Slot,
    last_reported_finalized: Slot,
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

        // Convert f64 tau to integer fraction: multiply by 10000 to preserve 4 decimal places
        let tau_numerator = (tau * 10000.0).round() as u128;
        let tau_denominator = 10000u128;

        HybridConsensus {
            validators: validators_map,
            total_stake,
            current_slot: 0,
            current_epoch: 0,
            epoch_randomness: H256::zero(),
            epoch_length,
            tau,
            tau_numerator,
            tau_denominator,
            my_vrf_keypair,
            my_bls_keypair,
            my_address,
            current_phase: Phase::Propose,
            votes: HashMap::new(),
            qcs: HashMap::new(),
            locked_block: None,
            locked_slot: 0,
            block_parents: HashMap::new(),
            block_slots: HashMap::new(),
            vote_record: HashMap::new(),
            vrf_pubkeys: HashMap::new(),
            bls_pubkeys: HashMap::new(),
            pacemaker: Pacemaker::new(Duration::from_millis(500)),
            committed_slot: 0,
            finalized_slot: 0,
            last_reported_finalized: 0,
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
        if check_leader_eligibility_integer(
            &proof.output,
            validator.stake,
            self.total_stake,
            self.tau_numerator,
            self.tau_denominator,
        ) {
            Some(proof)
        } else {
            None
        }
    }

    /// Register a validator's VRF public key for cross-validation.
    pub fn register_vrf_pubkey(&mut self, address: Address, vrf_pubkey: [u8; 32]) {
        self.vrf_pubkeys.insert(address, vrf_pubkey);
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

        let vrf_pubkey: [u8; 32] = *self.vrf_pubkeys.get(&proposer_addr).ok_or_else(|| {
            anyhow::anyhow!(
                "no VRF public key registered for proposer {:?}",
                proposer_addr
            )
        })?;

        if !verify_proof(&vrf_pubkey, &input, &vrf_proof)? {
            return Ok(false);
        }

        // Check eligibility threshold (using deterministic integer arithmetic)
        Ok(check_leader_eligibility_integer(
            &vrf_proof.output,
            validator.stake,
            self.total_stake,
            self.tau_numerator,
            self.tau_denominator,
        ))
    }

    /// Create a vote for a block (BLS signature)
    pub fn create_vote(&self, block_hash: H256, _phase: Phase) -> Result<Option<Vote>> {
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

        // Create vote message: block_hash || slot (standardized format)
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&self.current_slot.to_le_bytes());

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

    /// Register a validator's BLS public key for vote signature verification.
    ///
    /// Requires a valid proof-of-possession (PoP) signature to prevent rogue key attacks.
    /// The PoP proves the registrant knows the secret key corresponding to the public key.
    pub fn register_bls_pubkey(
        &mut self,
        address: Address,
        bls_pubkey: Vec<u8>,
        pop_signature: &[u8],
    ) -> Result<()> {
        if bls_pubkey.len() != 48 {
            bail!("BLS pubkey must be 48 bytes, got {}", bls_pubkey.len());
        }
        // Verify proof-of-possession to prevent rogue key attacks
        match aether_crypto_bls::verify_pop(&bls_pubkey, pop_signature)? {
            true => {}
            false => bail!(
                "invalid proof-of-possession for BLS pubkey registered by {:?}",
                address
            ),
        }
        self.bls_pubkeys.insert(address, bls_pubkey);
        Ok(())
    }

    /// Record a block's parent relationship (for 2-chain finality).
    pub fn record_block(&mut self, block_hash: H256, parent_hash: H256, slot: Slot) {
        self.block_parents.insert(block_hash, parent_hash);
        self.block_slots.insert(block_hash, slot);
    }

    /// Update epoch randomness using a real VRF output from the first block.
    pub fn update_epoch_randomness(&mut self, block_vrf_output: &[u8; 32]) {
        let mut hasher = Sha256::new();
        hasher.update(self.epoch_randomness.as_bytes());
        hasher.update(block_vrf_output);
        hasher.update(self.current_epoch.to_le_bytes());
        self.epoch_randomness = H256::from_slice(&hasher.finalize()).unwrap();
    }

    /// Process a vote and check for quorum.
    ///
    /// Safety properties enforced:
    /// 1. Vote deduplication — one vote per validator per (slot, phase, block)
    /// 2. Stake verification — claimed stake must match validator registry
    /// 3. Unknown validator rejection
    pub fn process_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>> {
        // Verify vote is for current slot
        if vote.slot != self.current_slot {
            bail!(
                "vote for wrong slot: got {}, expected {}",
                vote.slot,
                self.current_slot
            );
        }

        let voter_addr = vote.validator.to_address();

        // Verify voter is a known validator
        let registered = self
            .validators
            .get(&voter_addr)
            .ok_or_else(|| anyhow::anyhow!("unknown validator: {:?}", voter_addr))?;

        // Verify claimed stake matches registry
        if vote.stake != registered.stake {
            bail!(
                "claimed stake {} != registered stake {} for {:?}",
                vote.stake,
                registered.stake,
                voter_addr
            );
        }

        // Verify BLS signature FIRST (before equivocation check, to prevent
        // an attacker from poisoning the equivocation record with invalid-sig votes).
        // Mandatory: every validator MUST have a registered BLS key, and every vote
        // MUST carry a valid 96-byte BLS signature.
        let bls_pk = self.bls_pubkeys.get(&voter_addr).ok_or_else(|| {
            anyhow::anyhow!(
                "no BLS public key registered for validator {:?}",
                voter_addr
            )
        })?;
        if bls_pk.len() != 48 {
            bail!(
                "registered BLS pubkey has invalid length {} for {:?}",
                bls_pk.len(),
                voter_addr
            );
        }
        let vote_msg = {
            let mut msg = Vec::new();
            msg.extend_from_slice(vote.block_hash.as_bytes());
            msg.extend_from_slice(&vote.slot.to_le_bytes());
            msg
        };
        let sig_bytes = vote.signature.as_bytes();
        if sig_bytes.len() != 96 {
            bail!(
                "vote signature has invalid length {} from {:?}",
                sig_bytes.len(),
                voter_addr
            );
        }
        match aether_crypto_bls::keypair::verify(bls_pk, &vote_msg, sig_bytes) {
            Ok(true) => {} // Valid signature
            Ok(false) => bail!("invalid BLS signature on vote from {:?}", voter_addr),
            Err(e) => bail!("BLS verification error for {:?}: {e}", voter_addr),
        }

        // Equivocation detection: check if this validator already voted for a DIFFERENT block
        // at this slot. Only checked AFTER signature verification so invalid-sig votes
        // can't poison the record.
        // Uses entry() API for atomic check-then-insert (no TOCTOU race).
        let vote_key = (vote.slot, voter_addr);
        match self.vote_record.entry(vote_key) {
            Entry::Occupied(e) => {
                if *e.get() != vote.block_hash {
                    println!(
                        "⚠ EQUIVOCATION: validator {:?} voted for {:?} AND {:?} at slot {}",
                        voter_addr,
                        e.get(),
                        vote.block_hash,
                        vote.slot
                    );
                    bail!(
                        "equivocation detected: validator {:?} double-voted at slot {}",
                        voter_addr,
                        vote.slot
                    );
                }
            }
            Entry::Vacant(e) => {
                e.insert(vote.block_hash);
            }
        }

        // Bound vote storage: limit unique block hashes per (slot, phase) to prevent
        // memory exhaustion from adversarial blocks. Validators can propose at most
        // validator_count blocks, so 2x that is generous.
        let max_block_hashes_per_phase = self.validators.len() * 2;
        let existing_hashes: usize = self
            .votes
            .keys()
            .filter(|(s, p, _)| *s == vote.slot && *p == self.current_phase)
            .count();
        let key = (vote.slot, self.current_phase.clone(), vote.block_hash);
        if !self.votes.contains_key(&key) && existing_hashes >= max_block_hashes_per_phase {
            bail!(
                "too many block candidates for slot {} (limit: {})",
                vote.slot,
                max_block_hashes_per_phase
            );
        }

        // Deduplicate: one vote per validator per (slot, phase, block)
        let votes_map = self.votes.entry(key.clone()).or_default();
        if votes_map.contains_key(&voter_addr) {
            return Ok(None);
        }
        votes_map.insert(voter_addr, vote.clone());

        // Check for quorum (2/3+ stake)
        let voted_stake: u128 = votes_map.values().map(|v| v.stake).sum();
        let has_quorum = crate::has_quorum(voted_stake, self.total_stake);

        if has_quorum {
            // Single-validator fast path
            if self.validators.len() == 1 {
                let qc = QuorumCertificate {
                    slot: vote.slot,
                    block_hash: vote.block_hash,
                    phase: self.current_phase.clone(),
                    total_stake: vote.stake,
                    signers: vec![voter_addr],
                    aggregated_signature: vote.signature.as_bytes().to_vec(),
                    aggregated_pubkey: vec![],
                };
                self.qcs.insert(key, qc.clone());
                self.committed_slot = vote.slot;
                self.finalized_slot = vote.slot;
                self.pacemaker.on_commit();
                return Ok(Some(qc));
            }

            // Multi-validator: aggregate votes
            let votes_vec: Vec<Vote> = votes_map.values().cloned().collect();
            let qc = self.aggregate_votes(&votes_vec)?;
            self.qcs.insert(key, qc.clone());

            // Handle phase transitions with correct 2-chain finality
            match self.current_phase {
                Phase::Propose => {
                    // QC formed in Propose phase → advance to Prevote
                }
                Phase::Prevote => {
                    // Prevote QC formed → lock on this block
                    self.locked_block = Some(vote.block_hash);
                    self.locked_slot = vote.slot;
                }
                Phase::Precommit => {
                    // Precommit QC formed → check 2-chain finality rule:
                    // If C's parent (block B) has a prevote QC, finalize B.
                    if let Some(parent_hash) = self.block_parents.get(&vote.block_hash) {
                        if let Some(&parent_slot) = self.block_slots.get(parent_hash) {
                            let prevote_key = (parent_slot, Phase::Prevote, *parent_hash);
                            if self.qcs.contains_key(&prevote_key) {
                                self.finalized_slot = parent_slot;
                                println!(
                                    "FINALIZED slot {} (parent of slot {} block)",
                                    parent_slot, vote.slot
                                );
                            }
                        }
                    }
                    self.committed_slot = vote.slot;
                }
                Phase::Commit => {}
            }

            // CRITICAL: Advance phase after QC formation.
            // This drives the HotStuff state machine:
            // Propose → Prevote → Precommit → Commit → Propose
            self.advance_phase();
            self.pacemaker.on_commit();
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

        // Use registered BLS public keys (48 bytes) — NOT Ed25519 keys (32 bytes)
        let pubkeys: Vec<Vec<u8>> = votes
            .iter()
            .map(|v| {
                let addr = v.validator.to_address();
                // First try BLS pubkey registry (correct 48-byte keys)
                if let Some(bls_pk) = self.bls_pubkeys.get(&addr) {
                    bls_pk.clone()
                } else {
                    // Fallback: pad Ed25519 key (will produce invalid BLS verification
                    // but prevents panic — votes from unregistered validators were
                    // already rejected by process_vote)
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

        // Prune old consensus state to bound memory growth.
        // Keep only data from the last 200 slots.
        let prune_before = self.current_slot.saturating_sub(200);
        self.vote_record
            .retain(|(slot, _), _| *slot >= prune_before);
        self.qcs.retain(|(slot, _, _), _| *slot >= prune_before);
        // Prune block parent tracking for very old blocks
        self.block_slots.retain(|_, slot| *slot >= prune_before);
        let slots_to_keep: std::collections::HashSet<&H256> = self.block_slots.keys().collect();
        self.block_parents.retain(|k, _| slots_to_keep.contains(k));

        // Check for epoch transition
        if self.current_slot % self.epoch_length == 0 {
            // Default epoch randomness update (deterministic fallback).
            // In production, Node calls update_epoch_randomness() with a real VRF output
            // from the first finalized block of the new epoch, which overrides this.
            let mut hasher = Sha256::new();
            hasher.update(self.epoch_randomness.as_bytes());
            hasher.update(self.current_slot.to_le_bytes());
            hasher.update(self.current_epoch.to_le_bytes());
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

        // Validate timestamp: must be reasonable (within 1 hour of expected)
        let expected_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let max_drift: u64 = 3600; // 1 hour tolerance
        if block.header.timestamp > expected_timestamp + max_drift {
            bail!(
                "block timestamp {} is too far in the future (now={})",
                block.header.timestamp,
                expected_timestamp
            );
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
        // Only report true when a slot NEWLY becomes finalized
        if slot <= self.finalized_slot && slot > self.last_reported_finalized {
            self.last_reported_finalized = slot;
            true
        } else {
            false
        }
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

    fn record_block(&mut self, block_hash: H256, parent_hash: H256, slot: Slot) {
        self.block_parents.insert(block_hash, parent_hash);
        self.block_slots.insert(block_hash, slot);
    }

    fn update_epoch_randomness(&mut self, vrf_output: &[u8; 32]) {
        HybridConsensus::update_epoch_randomness(self, vrf_output);
    }

    fn validator_stake(&self, address: &Address) -> u128 {
        self.validators.get(address).map_or(0, |v| v.stake)
    }

    fn is_timed_out(&self) -> bool {
        self.pacemaker.is_timed_out()
    }

    fn on_timeout(&mut self) {
        self.pacemaker.on_timeout();
        // On timeout, reset to Propose phase for the next slot.
        // Simply advancing one phase would leave the node in a stale phase
        // (e.g., Precommit→Commit with no precommit QC), breaking liveness.
        self.current_phase = Phase::Propose;
        self.votes.clear();
    }

    fn register_bls_pubkey(
        &mut self,
        address: aether_types::Address,
        bls_pubkey: Vec<u8>,
        pop_signature: &[u8],
    ) -> Result<()> {
        self.register_bls_pubkey(address, bls_pubkey, pop_signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;

    fn create_test_validator(stake: u128) -> ValidatorInfo {
        let keypair = Keypair::generate();
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake,
            commission: 0,
            active: true,
        }
    }

    /// Create a test validator with an associated BLS keypair for signing votes.
    fn create_test_validator_with_bls(stake: u128) -> (ValidatorInfo, BlsKeypair) {
        let keypair = Keypair::generate();
        let bls_kp = BlsKeypair::generate();
        let vi = ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake,
            commission: 0,
            active: true,
        };
        (vi, bls_kp)
    }

    /// Register BLS keys for validators and create a signed vote.
    fn make_signed_vote(
        consensus: &mut HybridConsensus,
        vi: &ValidatorInfo,
        bls_kp: &BlsKeypair,
        block_hash: H256,
        slot: Slot,
    ) -> Vote {
        let addr = vi.pubkey.to_address();
        // Register BLS key with proof-of-possession if not already registered
        let pop = bls_kp.proof_of_possession();
        let _ = consensus.register_bls_pubkey(addr, bls_kp.public_key(), &pop);
        // Sign the vote message: block_hash || slot
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&slot.to_le_bytes());
        let sig = bls_kp.sign(&msg);
        Vote {
            slot,
            block_hash,
            validator: vi.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(sig),
            stake: vi.stake,
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
        assert_eq!(consensus.total_stake, 3000);
    }

    #[test]
    fn test_vote_deduplication_rejects_duplicate() {
        // 4 validators — no single validator can reach quorum (2/3 of 4000 = 2667)
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, _bls2) = create_test_validator_with_bls(1000);
        let (v3, _bls3) = create_test_validator_with_bls(1000);
        let (v4, _bls4) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone(), v4.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let vote = make_signed_vote(&mut consensus, &v1, &bls1, block_hash, 0);

        // First vote: accepted (no quorum yet)
        let result1 = consensus.process_vote(vote.clone());
        assert!(
            result1.is_ok(),
            "First vote should be accepted: {:?}",
            result1.err()
        );

        // Second identical vote from same validator: silently ignored (dedup)
        let result2 = consensus.process_vote(vote.clone()).unwrap();
        assert!(
            result2.is_none(),
            "Duplicate vote should return None (ignored)"
        );

        // Verify only 1 vote counted
        let key = (0, Phase::Propose, block_hash);
        let votes = consensus.votes.get(&key).unwrap();
        assert_eq!(votes.len(), 1, "Only 1 unique vote should be stored");
    }

    #[test]
    fn test_vote_rejects_inflated_stake() {
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);

        // Register BLS key, create a signed vote, then tamper with stake
        let mut vote = make_signed_vote(
            &mut consensus,
            &v1,
            &bls1,
            H256::from_slice(&[1u8; 32]).unwrap(),
            0,
        );
        vote.stake = 999_999; // Inflated stake (registered = 1000)

        let result = consensus.process_vote(vote);
        assert!(result.is_err(), "Inflated stake should be rejected");
    }

    #[test]
    fn test_vote_rejects_unknown_validator() {
        let v1 = create_test_validator(1000);
        let unknown = create_test_validator(5000);
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 100, None, None, None);

        let vote = Vote {
            slot: 0,
            block_hash: H256::from_slice(&[1u8; 32]).unwrap(),
            validator: unknown.pubkey.clone(), // Not in validator set
            signature: aether_types::Signature::from_bytes(vec![0u8; 96]),
            stake: unknown.stake,
        };

        let result = consensus.process_vote(vote);
        assert!(result.is_err(), "Unknown validator vote should be rejected");
    }

    #[test]
    fn test_byzantine_cannot_forge_quorum_with_duplicates() {
        // 3 validators with equal stake — quorum requires 2/3 = 2000 out of 3000
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, _bls2) = create_test_validator_with_bls(1000);
        let (v3, _bls3) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let vote = make_signed_vote(&mut consensus, &v1, &bls1, block_hash, 0);

        // Byzantine validator v1 submits vote 1000 times
        for _ in 0..1000 {
            let _ = consensus.process_vote(vote.clone());
        }

        // Only 1 vote should be counted (1000 stake, not 1,000,000)
        let key = (0, Phase::Propose, block_hash);
        let votes = consensus.votes.get(&key).unwrap();
        assert_eq!(
            votes.len(),
            1,
            "Dedup should prevent duplicate accumulation"
        );

        let voted_stake: u128 = votes.values().map(|v| v.stake).sum();
        assert_eq!(
            voted_stake, 1000,
            "Total stake should be 1000, not 1,000,000"
        );
        // Quorum not reached (1000 < 2000)
    }

    #[test]
    fn test_block_parent_tracking() {
        let v1 = create_test_validator(1000);
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 100, None, None, None);

        let parent_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let child_hash = H256::from_slice(&[2u8; 32]).unwrap();

        consensus.record_block(child_hash, parent_hash, 5);

        assert_eq!(consensus.block_parents.get(&child_hash), Some(&parent_hash));
        assert_eq!(consensus.block_slots.get(&child_hash), Some(&5));
    }

    #[test]
    fn test_epoch_randomness_update() {
        let v1 = create_test_validator(1000);
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 100, None, None, None);

        let initial_randomness = consensus.epoch_randomness;

        // Update with VRF output
        let vrf_output = [42u8; 32];
        consensus.update_epoch_randomness(&vrf_output);

        assert_ne!(
            consensus.epoch_randomness, initial_randomness,
            "Randomness should change after VRF update"
        );

        // Different VRF output → different randomness
        let mut consensus2 = HybridConsensus::new(
            vec![create_test_validator(1000)],
            0.8,
            100,
            None,
            None,
            None,
        );
        consensus2.update_epoch_randomness(&[99u8; 32]);

        assert_ne!(
            consensus.epoch_randomness, consensus2.epoch_randomness,
            "Different VRF outputs should produce different randomness"
        );
    }

    #[test]
    fn test_equivocation_detection_rejects_double_vote() {
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, _bls2) = create_test_validator_with_bls(1000);
        let (v3, _bls3) = create_test_validator_with_bls(1000);
        let (v4, _bls4) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone(), v4.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        let block_a = H256::from_slice(&[1u8; 32]).unwrap();
        let block_b = H256::from_slice(&[2u8; 32]).unwrap();

        // v1 votes for block A
        let vote_a = make_signed_vote(&mut consensus, &v1, &bls1, block_a, 0);
        assert!(consensus.process_vote(vote_a).is_ok());

        // v1 tries to vote for block B at the same slot — EQUIVOCATION!
        let vote_b = make_signed_vote(&mut consensus, &v1, &bls1, block_b, 0);
        let result = consensus.process_vote(vote_b);
        assert!(
            result.is_err(),
            "Double-voting for different blocks at same slot must be rejected"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("equivocation"),
            "Error should mention equivocation, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_timestamp_validation() {
        let v1 = create_test_validator(1000);
        let consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);

        // Block with timestamp far in the future should be rejected
        let mut block = Block::new(
            0,
            H256::zero(),
            v1.pubkey.to_address(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );
        // Set timestamp to year 2099
        block.header.timestamp = 4_000_000_000;

        let result = consensus.validate_block(&block);
        assert!(
            result.is_err(),
            "Block with future timestamp should be rejected"
        );
    }

    #[test]
    fn test_vote_rejected_without_bls_key() {
        let v1 = create_test_validator(1000);
        let v2 = create_test_validator(1000);
        let mut consensus =
            HybridConsensus::new(vec![v1.clone(), v2.clone()], 0.8, 100, None, None, None);
        // Do NOT register BLS key for v1
        let vote = Vote {
            slot: 0,
            block_hash: H256::from_slice(&[1u8; 32]).unwrap(),
            validator: v1.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(vec![0u8; 96]),
            stake: v1.stake,
        };
        let result = consensus.process_vote(vote);
        assert!(result.is_err(), "Vote without BLS key should be rejected");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no BLS public key"));
    }

    #[test]
    fn test_vote_rejected_with_wrong_signature_length() {
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);
        let pop = bls1.proof_of_possession();
        consensus
            .register_bls_pubkey(v1.pubkey.to_address(), bls1.public_key(), &pop)
            .unwrap();

        let vote = Vote {
            slot: 0,
            block_hash: H256::from_slice(&[1u8; 32]).unwrap(),
            validator: v1.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(vec![0u8; 64]), // Wrong: 64 instead of 96
            stake: v1.stake,
        };
        let result = consensus.process_vote(vote);
        assert!(
            result.is_err(),
            "Vote with wrong sig length should be rejected"
        );
        assert!(result.unwrap_err().to_string().contains("invalid length"));
    }

    #[test]
    fn test_vote_rejected_with_invalid_bls_signature() {
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);
        let pop = bls1.proof_of_possession();
        consensus
            .register_bls_pubkey(v1.pubkey.to_address(), bls1.public_key(), &pop)
            .unwrap();

        // Sign a DIFFERENT message than what process_vote expects
        let wrong_sig = bls1.sign(b"completely wrong message");
        let vote = Vote {
            slot: 0,
            block_hash: H256::from_slice(&[1u8; 32]).unwrap(),
            validator: v1.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(wrong_sig),
            stake: v1.stake,
        };
        let result = consensus.process_vote(vote);
        assert!(
            result.is_err(),
            "Vote with invalid BLS sig should be rejected"
        );
    }

    #[test]
    fn test_equivocation_detected() {
        // Create 4 validators so no single validator can reach quorum
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, _bls2) = create_test_validator_with_bls(1000);
        let (v3, _bls3) = create_test_validator_with_bls(1000);
        let (v4, _bls4) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone(), v4.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        let block_a = H256::from_slice(&[0xAAu8; 32]).unwrap();
        let block_b = H256::from_slice(&[0xBBu8; 32]).unwrap();

        // v1 votes for block_a at slot 5
        for _ in 0..5 {
            consensus.advance_slot();
        }
        let vote_a = make_signed_vote(&mut consensus, &v1, &bls1, block_a, 5);
        assert!(
            consensus.process_vote(vote_a).is_ok(),
            "first vote should succeed"
        );

        // v1 tries to vote for block_b at the same slot 5 -- equivocation
        let vote_b = make_signed_vote(&mut consensus, &v1, &bls1, block_b, 5);
        let result = consensus.process_vote(vote_b);
        assert!(
            result.is_err(),
            "second vote for different block at same slot must be rejected"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("equivocation"),
            "error should mention equivocation, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_finality_not_reported_twice() {
        // Single-validator setup: quorum is immediate
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);

        // Advance to slot 1 so finalized_slot (1) > last_reported_finalized (0)
        consensus.advance_slot();

        let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let vote = make_signed_vote(&mut consensus, &v1, &bls1, block_hash, 1);

        // Process vote -- single validator reaches quorum immediately, finalizing slot 1
        let qc = consensus.process_vote(vote).unwrap();
        assert!(
            qc.is_some(),
            "single-validator quorum should form immediately"
        );

        // First check_finality for slot 1 should return true
        let first = consensus.check_finality(1);
        assert!(
            first,
            "first check_finality should return true for newly finalized slot"
        );

        // Second check_finality for the same slot should return false (already reported)
        let second = consensus.check_finality(1);
        assert!(
            !second,
            "second check_finality should return false -- finality already reported"
        );
    }

    #[test]
    fn test_equivocation_detected_slot1() {
        // Adversarial test: validator votes for block A then block B at slot 1
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, _bls2) = create_test_validator_with_bls(1000);
        let (v3, _bls3) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        // Advance to slot 1
        consensus.advance_slot();

        let block_a = H256::from_slice(&[0xAAu8; 32]).unwrap();
        let block_b = H256::from_slice(&[0xBBu8; 32]).unwrap();

        // Vote for block A at slot 1
        let vote_a = make_signed_vote(&mut consensus, &v1, &bls1, block_a, 1);
        assert!(
            consensus.process_vote(vote_a).is_ok(),
            "first vote should succeed"
        );

        // Vote for block B at slot 1 from the same validator -- equivocation
        let vote_b = make_signed_vote(&mut consensus, &v1, &bls1, block_b, 1);
        let result = consensus.process_vote(vote_b);
        assert!(
            result.is_err(),
            "second vote for different block should be rejected"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("equivocation"),
            "error should contain 'equivocation', got: {}",
            err_msg
        );
    }

    #[test]
    fn test_finality_not_double_reported() {
        // check_finality should return true only once per slot
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);

        // Advance to slot 1 and finalize via single-validator quorum
        consensus.advance_slot();
        let block_hash = H256::from_slice(&[0xCCu8; 32]).unwrap();
        let vote = make_signed_vote(&mut consensus, &v1, &bls1, block_hash, 1);
        let qc = consensus.process_vote(vote).unwrap();
        assert!(qc.is_some(), "single-validator should reach quorum");

        // First call: true (newly finalized)
        let first = consensus.check_finality(1);
        assert!(first, "first check_finality should return true");

        // Second call: false (already reported)
        let second = consensus.check_finality(1);
        assert!(!second, "second check_finality should return false");
    }
}
