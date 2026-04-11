// ============================================================================
// HYBRID CONSENSUS - Phase 1 Full Integration
// ============================================================================
// Combines VRF-PoS leader election + HotStuff BFT + BLS signature aggregation
// ============================================================================

use crate::{ConsensusEngine, Pacemaker};
use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, BlsKeypair};
use aether_crypto_vrf::{
    check_leader_eligibility_integer, EcVrfVerifier, VrfKeypair, VrfProof, VrfSigner, VrfVerifier,
};
use aether_types::{Address, Block, PublicKey, Slot, ValidatorInfo, Vote, H256};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::Duration;

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
    /// Live validator set (updated by slashing/staking mid-epoch).
    validators: HashMap<Address, ValidatorInfo>,
    total_stake: u128,

    /// Epoch-frozen validator snapshot used for leader election.
    /// Prevents mid-epoch stake changes from altering leader schedules.
    epoch_validators: HashMap<Address, ValidatorInfo>,
    epoch_total_stake: u128,

    // === Slot/Epoch Management ===
    current_slot: Slot,
    current_epoch: u64,
    epoch_randomness: H256,
    epoch_length: u64,
    /// Whether epoch randomness has been updated from a real VRF output this epoch.
    epoch_randomness_updated: bool,

    // === VRF-PoS Parameters ===
    #[allow(dead_code)]
    tau: f64, // Leader rate (0 < tau <= 1) — kept for API compatibility
    tau_numerator: u128, // Integer numerator for deterministic eligibility check
    tau_denominator: u128, // Integer denominator for deterministic eligibility check
    my_vrf_keypair: Option<Box<dyn VrfSigner>>,
    vrf_verifier: Box<dyn VrfVerifier>,
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
        Self::with_vrf(
            validators,
            tau,
            epoch_length,
            my_vrf_keypair.map(|k| Box::new(k) as Box<dyn VrfSigner>),
            Box::new(EcVrfVerifier),
            my_bls_keypair,
            my_address,
        )
    }

    pub fn with_vrf(
        validators: Vec<ValidatorInfo>,
        tau: f64,
        epoch_length: u64,
        my_vrf_keypair: Option<Box<dyn VrfSigner>>,
        vrf_verifier: Box<dyn VrfVerifier>,
        my_bls_keypair: Option<BlsKeypair>,
        my_address: Option<Address>,
    ) -> Self {
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

        HybridConsensus {
            epoch_validators: validators_map.clone(),
            epoch_total_stake: total_stake,
            validators: validators_map,
            total_stake,
            current_slot: 0,
            current_epoch: 0,
            epoch_randomness: H256::zero(),
            epoch_length,
            epoch_randomness_updated: false,
            tau,
            tau_numerator,
            tau_denominator,
            my_vrf_keypair,
            vrf_verifier,
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
        // Use epoch-frozen validator set for deterministic leader election.
        let validator = self.epoch_validators.get(my_addr)?;

        // Compute VRF input: epoch_randomness || slot
        let mut input = Vec::new();
        input.extend_from_slice(self.epoch_randomness.as_bytes());
        input.extend_from_slice(&slot.to_le_bytes());

        let proof = VrfSigner::prove(vrf_keypair.as_ref(), &input);

        // Check eligibility threshold against epoch-frozen stake
        if check_leader_eligibility_integer(
            &proof.output,
            validator.stake,
            self.epoch_total_stake,
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
        // Use epoch-frozen validator set for verification consistency.
        let validator = self
            .epoch_validators
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

        if !self.vrf_verifier.verify(&vrf_pubkey, &input, &vrf_proof)? {
            return Ok(false);
        }

        Ok(check_leader_eligibility_integer(
            &vrf_proof.output,
            validator.stake,
            self.epoch_total_stake,
            self.tau_numerator,
            self.tau_denominator,
        ))
    }

    /// Create a vote for a block (BLS signature)
    pub fn create_vote(&self, block_hash: H256, _phase: Phase) -> Result<Option<Vote>> {
        let _span = tracing::debug_span!(
            "create_vote",
            slot = self.current_slot,
            block = ?block_hash,
        )
        .entered();

        let bls_keypair = match &self.my_bls_keypair {
            Some(kp) => kp,
            None => return Ok(None), // Not a validator
        };

        let my_addr = match &self.my_address {
            Some(addr) => addr,
            None => return Ok(None),
        };

        let validator = self
            .epoch_validators
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

    /// Update epoch randomness using a real VRF output from the first finalized block.
    /// Returns true if this was the first update this epoch (idempotent guard).
    pub fn update_epoch_randomness(&mut self, block_vrf_output: &[u8; 32]) -> bool {
        if self.epoch_randomness_updated {
            return false;
        }
        let mut hasher = Sha256::new();
        hasher.update(self.epoch_randomness.as_bytes());
        hasher.update(block_vrf_output);
        hasher.update(self.current_epoch.to_le_bytes());
        self.epoch_randomness = H256::from(<[u8; 32]>::from(hasher.finalize()));
        self.epoch_randomness_updated = true;
        true
    }

    /// Whether epoch randomness has already been updated from a real VRF output.
    pub fn epoch_randomness_updated(&self) -> bool {
        self.epoch_randomness_updated
    }

    /// Process a vote and check for quorum.
    ///
    /// Safety properties enforced:
    /// 1. Vote deduplication — one vote per validator per (slot, phase, block)
    /// 2. Stake verification — claimed stake must match validator registry
    /// 3. Unknown validator rejection
    pub fn process_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>> {
        let _span = tracing::debug_span!(
            "process_vote",
            slot = vote.slot,
            block = ?vote.block_hash,
            voter = ?vote.validator.to_address(),
            stake = vote.stake,
        )
        .entered();

        // Verify vote is for current slot
        if vote.slot != self.current_slot {
            bail!(
                "vote for wrong slot: got {}, expected {}",
                vote.slot,
                self.current_slot
            );
        }

        let voter_addr = vote.validator.to_address();

        // Verify voter is a known validator in the epoch-frozen set.
        // Using the epoch snapshot ensures mid-epoch slashing doesn't change
        // who can vote or their stake weight within the current epoch.
        let registered = self
            .epoch_validators
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
                    tracing::warn!(
                        validator = ?voter_addr,
                        first_block = ?e.get(),
                        second_block = ?vote.block_hash,
                        slot = vote.slot,
                        "EQUIVOCATION: validator double-voted in same slot"
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
        let voted_stake: u128 = votes_map
            .values()
            .map(|v| v.stake)
            .fold(0u128, u128::saturating_add);
        // Use epoch-frozen total stake for quorum calculation so mid-epoch
        // slashing cannot lower the quorum threshold within an epoch.
        let has_quorum = crate::has_quorum(voted_stake, self.epoch_total_stake);

        if has_quorum {
            // Single-validator fast path
            if self.epoch_validators.len() == 1 {
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
                if vote.slot > self.committed_slot {
                    self.committed_slot = vote.slot;
                }
                if vote.slot > self.finalized_slot {
                    self.finalized_slot = vote.slot;
                }
                self.pacemaker.on_commit();
                return Ok(Some(qc));
            }

            // Multi-validator: aggregate votes
            let votes_vec: Vec<Vote> = votes_map.values().cloned().collect();
            let qc = self.aggregate_votes(&votes_vec)?;
            self.qcs.insert(key, qc.clone());

            // 2-CHAIN FINALITY RULE
            //
            // Because advance_slot() resets the phase to Propose every slot,
            // only Propose-phase QCs ever form in multi-validator mode.
            // The Prevote/Precommit/Commit branches below are kept for
            // correctness if the protocol later adds intra-slot phase
            // progression, but the Propose branch now carries the
            // production finality logic:
            //
            //   Block B is finalized when:
            //     1. B has a QC (from its own slot's Propose phase)
            //     2. B's child C also has a QC (current slot's Propose phase)
            //     3. C.parent_hash == B.hash
            //
            match self.current_phase {
                Phase::Propose => {
                    // QC formed for this block (child C).  Check if parent
                    // block B already has a QC — if so, B is finalized.
                    if let Some(parent_hash) = self.block_parents.get(&vote.block_hash).copied() {
                        if let Some(&parent_slot) = self.block_slots.get(&parent_hash) {
                            // Parent's QC was also formed in Propose phase
                            let parent_key = (parent_slot, Phase::Propose, parent_hash);
                            if self.qcs.contains_key(&parent_key)
                                && parent_slot > self.finalized_slot
                            {
                                self.finalized_slot = parent_slot;
                                tracing::info!(
                                    finalized_slot = parent_slot,
                                    child_slot = vote.slot,
                                    "2-chain finality: block finalized via consecutive QCs"
                                );
                            }
                        }
                    }
                    // Lock on this block (it has a QC)
                    self.locked_block = Some(vote.block_hash);
                    self.locked_slot = vote.slot;
                    // Track committed slot
                    if vote.slot > self.committed_slot {
                        self.committed_slot = vote.slot;
                    }
                }
                Phase::Prevote => {
                    // Prevote QC formed → lock on this block
                    self.locked_block = Some(vote.block_hash);
                    self.locked_slot = vote.slot;
                }
                Phase::Precommit => {
                    // Precommit QC formed → check 2-chain finality rule
                    // (same logic, but keyed on Prevote QC for parent)
                    if let Some(parent_hash) = self.block_parents.get(&vote.block_hash).copied() {
                        if let Some(&parent_slot) = self.block_slots.get(&parent_hash) {
                            let prevote_key = (parent_slot, Phase::Prevote, parent_hash);
                            if self.qcs.contains_key(&prevote_key)
                                && parent_slot > self.finalized_slot
                            {
                                self.finalized_slot = parent_slot;
                                tracing::info!(
                                    finalized_slot = parent_slot,
                                    child_slot = vote.slot,
                                    "2-chain finality: slot finalized via precommit QC"
                                );
                            }
                        }
                    }
                    if vote.slot > self.committed_slot {
                        self.committed_slot = vote.slot;
                    }
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
        let _span = tracing::debug_span!(
            "aggregate_votes",
            num_votes = votes.len(),
            slot = votes.first().map(|v| v.slot).unwrap_or(0),
        )
        .entered();

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

        let total_stake = votes
            .iter()
            .map(|v| v.stake)
            .fold(0u128, u128::saturating_add);
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

impl crate::Finality for HybridConsensus {
    fn check_finality(&mut self, slot: Slot) -> bool {
        if slot <= self.finalized_slot && slot > self.last_reported_finalized {
            tracing::info!(
                slot,
                finalized_slot = self.finalized_slot,
                "finality confirmed"
            );
            self.last_reported_finalized = slot;
            true
        } else {
            false
        }
    }

    fn finalized_slot(&self) -> Slot {
        self.finalized_slot
    }

    fn record_block(&mut self, block_hash: H256, parent_hash: H256, slot: Slot) {
        self.block_parents.insert(block_hash, parent_hash);
        self.block_slots.insert(block_hash, slot);
    }
}

impl ConsensusEngine for HybridConsensus {
    fn current_slot(&self) -> Slot {
        self.current_slot
    }

    fn advance_slot(&mut self) {
        self.current_slot = self.current_slot.saturating_add(1);
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
        if self.epoch_length > 0 && self.current_slot % self.epoch_length == 0 {
            tracing::info!(
                slot = self.current_slot,
                new_epoch = self.current_epoch.saturating_add(1),
                validators = self.validators.len(),
                total_stake = self.total_stake,
                vrf_updated = self.epoch_randomness_updated,
                "epoch transition"
            );

            // If no real VRF output arrived this epoch, apply deterministic fallback.
            if !self.epoch_randomness_updated {
                let mut hasher = Sha256::new();
                hasher.update(self.epoch_randomness.as_bytes());
                hasher.update(self.current_slot.to_le_bytes());
                hasher.update(self.current_epoch.to_le_bytes());
                let new_randomness = hasher.finalize();
                self.epoch_randomness = H256::from(<[u8; 32]>::from(new_randomness));
            }
            self.epoch_randomness_updated = false;
            self.current_epoch = self.current_epoch.saturating_add(1);

            // Snapshot the current validator set for the new epoch.
            // Leader election uses this frozen snapshot so mid-epoch slashing
            // doesn't retroactively alter the leader schedule.
            self.epoch_validators = self.validators.clone();
            self.epoch_total_stake = self.total_stake;
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
        let _span = tracing::debug_span!(
            "validate_block",
            slot = block.header.slot,
            block = ?block.hash(),
            proposer = ?block.header.proposer,
        )
        .entered();

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

        // HotStuff safe node predicate: accept block if it extends the locked block,
        // OR if its parent is at a slot >= our locked slot (meaning a quorum has moved
        // past our lock, making it safe to vote for a new branch after a view change).
        // Without this, validators can deadlock after a timeout if the locked block's
        // chain stalls — the new leader's block extending a different branch would be
        // permanently rejected.
        if let Some(locked) = &self.locked_block {
            if block.header.parent_hash != *locked {
                let parent_slot = self
                    .block_slots
                    .get(&block.header.parent_hash)
                    .copied()
                    .unwrap_or(0);
                if parent_slot < self.locked_slot {
                    bail!(
                        "block does not extend locked block and parent slot {} < locked slot {} \
                         (safe node predicate failed)",
                        parent_slot,
                        self.locked_slot
                    );
                }
            }
        }

        Ok(())
    }

    fn add_vote(&mut self, vote: Vote) -> Result<()> {
        self.process_vote(vote)?;
        Ok(())
    }

    fn total_stake(&self) -> u128 {
        self.total_stake
    }

    fn get_leader_proof(&self, slot: Slot) -> Option<VrfProof> {
        self.check_my_eligibility(slot)
    }

    fn update_epoch_randomness(&mut self, vrf_output: &[u8; 32]) -> bool {
        HybridConsensus::update_epoch_randomness(self, vrf_output)
    }

    fn validator_stake(&self, address: &Address) -> u128 {
        self.validators.get(address).map_or(0, |v| v.stake)
    }

    fn is_timed_out(&self) -> bool {
        self.pacemaker.is_timed_out()
    }

    fn on_timeout(&mut self) {
        tracing::warn!(
            slot = self.current_slot,
            round = self.pacemaker.current_round(),
            phase = ?self.current_phase,
            finalized = self.finalized_slot,
            "consensus timeout — resetting phase"
        );
        self.pacemaker.on_timeout();
        // On timeout, reset to Propose phase for the next slot.
        // Simply advancing one phase would leave the node in a stale phase
        // (e.g., Precommit→Commit with no precommit QC), breaking liveness.
        self.current_phase = Phase::Propose;
        self.votes.clear();
    }

    fn advance_pacemaker_to_round(&mut self, round: u64) {
        self.pacemaker.advance_to_round(round);
    }

    fn get_bls_pubkey(&self, address: &aether_types::Address) -> Option<Vec<u8>> {
        self.bls_pubkeys.get(address).cloned()
    }

    fn register_bls_pubkey(
        &mut self,
        address: aether_types::Address,
        bls_pubkey: Vec<u8>,
        pop_signature: &[u8],
    ) -> Result<()> {
        self.register_bls_pubkey(address, bls_pubkey, pop_signature)
    }

    fn slash_validator(&mut self, address: &Address, slash_bps: u128) -> u128 {
        if let Some(validator) = self.validators.get_mut(address) {
            let slash_amount = mul_div(validator.stake, slash_bps, 10000);
            validator.stake = validator.stake.saturating_sub(slash_amount);
            self.total_stake = self.total_stake.saturating_sub(slash_amount);
            slash_amount
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Finality;
    use aether_crypto_primitives::Keypair;
    use aether_types::BlockHeader;

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

        let voted_stake: u128 = votes
            .values()
            .map(|v| v.stake)
            .fold(0u128, u128::saturating_add);
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

        // First update should succeed
        let vrf_output = [42u8; 32];
        assert!(consensus.update_epoch_randomness(&vrf_output));

        assert_ne!(
            consensus.epoch_randomness, initial_randomness,
            "Randomness should change after VRF update"
        );

        // Second update in same epoch should be rejected (idempotent guard)
        let randomness_after_first = consensus.epoch_randomness;
        assert!(!consensus.update_epoch_randomness(&[99u8; 32]));
        assert_eq!(
            consensus.epoch_randomness, randomness_after_first,
            "Second update in same epoch should be rejected"
        );

        // Different VRF output → different randomness (fresh consensus)
        let mut consensus2 = HybridConsensus::new(
            vec![create_test_validator(1000)],
            0.8,
            100,
            None,
            None,
            None,
        );
        assert!(consensus2.update_epoch_randomness(&[99u8; 32]));

        assert_ne!(
            consensus.epoch_randomness, consensus2.epoch_randomness,
            "Different VRF outputs should produce different randomness"
        );
    }

    #[test]
    fn test_epoch_stake_snapshot() {
        let v1 = create_test_validator(1000);
        let v1_addr = v1.pubkey.to_address();
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 10, None, None, None);

        // Initial epoch snapshot should match live set
        assert_eq!(consensus.epoch_total_stake, 1000);
        assert_eq!(
            consensus.epoch_validators.get(&v1_addr).unwrap().stake,
            1000
        );

        // Slash validator mid-epoch (live set changes)
        consensus.slash_validator(&v1_addr, 5000); // 50% slash
        assert_eq!(consensus.total_stake, 500);
        // Epoch snapshot should NOT change mid-epoch
        assert_eq!(consensus.epoch_total_stake, 1000);
        assert_eq!(
            consensus.epoch_validators.get(&v1_addr).unwrap().stake,
            1000
        );

        // Advance to epoch boundary (slot 10)
        for _ in 0..10 {
            consensus.advance_slot();
        }

        // Now epoch snapshot should reflect the slashed stake
        assert_eq!(consensus.epoch_total_stake, 500);
        assert_eq!(consensus.epoch_validators.get(&v1_addr).unwrap().stake, 500);
    }

    #[test]
    fn test_epoch_randomness_resets_across_epochs() {
        let v1 = create_test_validator(1000);
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 5, None, None, None);

        // Update randomness in epoch 0
        assert!(consensus.update_epoch_randomness(&[42u8; 32]));
        assert!(consensus.epoch_randomness_updated());

        // Advance to epoch boundary
        for _ in 0..5 {
            consensus.advance_slot();
        }

        // Flag should be reset for new epoch
        assert!(!consensus.epoch_randomness_updated());
        // Should be able to update again
        assert!(consensus.update_epoch_randomness(&[77u8; 32]));
        assert!(consensus.epoch_randomness_updated());
    }

    #[test]
    fn test_epoch_fallback_randomness_only_when_no_vrf() {
        let v1 = create_test_validator(1000);

        // Case 1: No VRF update — deterministic fallback applies at epoch boundary
        let mut c1 = HybridConsensus::new(vec![v1.clone()], 0.8, 5, None, None, None);
        let r_before = c1.epoch_randomness;
        for _ in 0..5 {
            c1.advance_slot();
        }
        assert_ne!(
            c1.epoch_randomness, r_before,
            "fallback should change randomness"
        );

        // Case 2: VRF update applied — fallback should NOT override at boundary
        let mut c2 = HybridConsensus::new(vec![v1], 0.8, 5, None, None, None);
        c2.update_epoch_randomness(&[42u8; 32]);
        let r_after_vrf = c2.epoch_randomness;
        for _ in 0..5 {
            c2.advance_slot();
        }
        // The VRF-derived randomness should have been preserved through the boundary
        // (fallback skipped because epoch_randomness_updated was true).
        assert_eq!(
            c2.epoch_randomness, r_after_vrf,
            "VRF randomness should be preserved — fallback must not override"
        );
        // And it should differ from the fallback-only path
        assert_ne!(
            c1.epoch_randomness, c2.epoch_randomness,
            "VRF-seeded and fallback-seeded epochs should diverge"
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

    #[test]
    fn test_slash_validator_reduces_stake() {
        let v1 = create_test_validator(1_000_000);
        let addr = v1.pubkey.to_address();
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 100, None, None, None);

        assert_eq!(consensus.validator_stake(&addr), 1_000_000);
        let initial_total = consensus.total_stake();

        // Slash 5% (500 bps)
        let slashed = consensus.slash_validator(&addr, 500);
        assert_eq!(slashed, 50_000);
        assert_eq!(consensus.validator_stake(&addr), 950_000);
        assert_eq!(consensus.total_stake(), initial_total - 50_000);
    }

    #[test]
    fn test_slash_unknown_validator_returns_zero() {
        let v1 = create_test_validator(1_000_000);
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 100, None, None, None);

        let unknown = Address::from_slice(&[0xFFu8; 20]).unwrap();
        assert_eq!(consensus.slash_validator(&unknown, 500), 0);
    }

    #[test]
    fn test_slash_validator_saturates_at_zero() {
        let v1 = create_test_validator(100);
        let addr = v1.pubkey.to_address();
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 100, None, None, None);

        // Slash 100% (10000 bps)
        let slashed = consensus.slash_validator(&addr, 10000);
        assert_eq!(slashed, 100);
        assert_eq!(consensus.validator_stake(&addr), 0);

        // Slash again — nothing left
        let slashed2 = consensus.slash_validator(&addr, 500);
        assert_eq!(slashed2, 0);
    }

    #[test]
    fn test_finality_monotonicity_never_regresses() {
        // In a single-validator setup, finalized_slot advances monotonically.
        // Even if a late vote arrives for an older slot, finalized_slot must not decrease.
        let v1 = create_test_validator(1_000_000);
        let bls1 = BlsKeypair::generate();
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);

        // Advance to slot 3 and finalize it
        consensus.advance_slot(); // slot 1
        consensus.advance_slot(); // slot 2
        consensus.advance_slot(); // slot 3

        let block3 = H256::from_slice(&[0x33u8; 32]).unwrap();
        let vote3 = make_signed_vote(&mut consensus, &v1, &bls1, block3, 3);
        let qc3 = consensus.process_vote(vote3).unwrap();
        assert!(qc3.is_some(), "should reach quorum at slot 3");
        assert_eq!(consensus.finalized_slot(), 3);
        assert_eq!(consensus.committed_slot, 3);

        // Verify finalized_slot == 3 after processing
        let finalized_before = consensus.finalized_slot();

        // Advance to slot 4 and finalize
        consensus.advance_slot(); // slot 4
        let block4 = H256::from_slice(&[0x44u8; 32]).unwrap();
        let vote4 = make_signed_vote(&mut consensus, &v1, &bls1, block4, 4);
        let qc4 = consensus.process_vote(vote4).unwrap();
        assert!(qc4.is_some());
        assert!(
            consensus.finalized_slot() >= finalized_before,
            "finalized_slot must never decrease: was {}, now {}",
            finalized_before,
            consensus.finalized_slot()
        );
        assert_eq!(consensus.finalized_slot(), 4);
    }

    #[test]
    fn test_committed_slot_monotonicity() {
        // committed_slot must never decrease even with out-of-order QC processing
        let v1 = create_test_validator(1_000_000);
        let bls1 = BlsKeypair::generate();
        let mut consensus = HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, None, None);

        consensus.advance_slot(); // slot 1
        let block1 = H256::from_slice(&[0x11u8; 32]).unwrap();
        let vote1 = make_signed_vote(&mut consensus, &v1, &bls1, block1, 1);
        consensus.process_vote(vote1).unwrap();
        assert_eq!(consensus.committed_slot, 1);

        consensus.advance_slot(); // slot 2
        let block2 = H256::from_slice(&[0x22u8; 32]).unwrap();
        let vote2 = make_signed_vote(&mut consensus, &v1, &bls1, block2, 2);
        consensus.process_vote(vote2).unwrap();
        assert!(
            consensus.committed_slot >= 1,
            "committed_slot must not regress"
        );
    }

    // --- Safe node predicate tests ---

    /// Helper: create a HybridConsensus with a single validator that is always
    /// eligible (tau=1.0, 100% stake). Returns (consensus, vrf_keypair, proposer_address).
    fn create_single_validator_consensus() -> (HybridConsensus, VrfKeypair, Address) {
        let vrf_kp = VrfKeypair::generate();
        let ed_kp = Keypair::generate();
        let pubkey = PublicKey::from_bytes(ed_kp.public_key());
        let addr = pubkey.to_address();
        let vi = ValidatorInfo {
            pubkey,
            stake: 1_000_000,
            commission: 0,
            active: true,
        };
        let mut consensus =
            HybridConsensus::new(vec![vi], 1.0, 100, Some(vrf_kp.clone()), None, Some(addr));
        consensus.register_vrf_pubkey(addr, *vrf_kp.public_key());
        (consensus, vrf_kp, addr)
    }

    /// Build a block at `slot` with the given `parent_hash` and a valid VRF proof.
    fn make_valid_block(
        consensus: &HybridConsensus,
        vrf_kp: &VrfKeypair,
        proposer: Address,
        slot: Slot,
        parent_hash: H256,
    ) -> Block {
        let mut input = Vec::new();
        input.extend_from_slice(consensus.epoch_randomness.as_bytes());
        input.extend_from_slice(&slot.to_le_bytes());
        let proof = vrf_kp.prove(&input);
        Block {
            header: BlockHeader {
                version: aether_types::PROTOCOL_VERSION,
                slot,
                parent_hash,
                state_root: H256::zero(),
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                proposer,
                vrf_proof: aether_types::VrfProof {
                    output: proof.output,
                    proof: proof.proof,
                },
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
            transactions: vec![],
            aggregated_vote: None,
            slash_evidence: vec![],
        }
    }

    #[test]
    fn test_validate_block_no_lock_accepts_any_parent() {
        let (mut consensus, vrf_kp, proposer) = create_single_validator_consensus();
        consensus.current_slot = 5;
        let block = make_valid_block(
            &consensus,
            &vrf_kp,
            proposer,
            3,
            H256::from_slice(&[0xAA; 32]).unwrap(),
        );
        // No lock set — should accept any parent
        assert!(consensus.validate_block(&block).is_ok());
    }

    #[test]
    fn test_validate_block_locked_accepts_extending_block() {
        let (mut consensus, vrf_kp, proposer) = create_single_validator_consensus();
        let locked_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 2;
        consensus.current_slot = 5;

        // Block extends the locked block — should be accepted
        let block = make_valid_block(&consensus, &vrf_kp, proposer, 3, locked_hash);
        assert!(consensus.validate_block(&block).is_ok());
    }

    #[test]
    fn test_validate_block_locked_rejects_lower_parent_slot() {
        let (mut consensus, vrf_kp, proposer) = create_single_validator_consensus();
        let locked_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 5;
        consensus.current_slot = 10;

        // Parent is at slot 3, which is BELOW our locked_slot of 5
        let other_parent = H256::from_slice(&[0xCC; 32]).unwrap();
        consensus.block_slots.insert(other_parent, 3);

        let block = make_valid_block(&consensus, &vrf_kp, proposer, 7, other_parent);
        let result = consensus.validate_block(&block);
        assert!(
            result.is_err(),
            "should reject: parent slot 3 < locked slot 5"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("safe node predicate"),
            "error message should mention safe node predicate"
        );
    }

    #[test]
    fn test_validate_block_safe_unlock_accepts_higher_parent_slot() {
        let (mut consensus, vrf_kp, proposer) = create_single_validator_consensus();
        let locked_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 3;
        consensus.current_slot = 10;

        // Parent is at slot 5, which is ABOVE our locked_slot of 3.
        // This justifies unlocking — the network has moved past our lock.
        let other_parent = H256::from_slice(&[0xDD; 32]).unwrap();
        consensus.block_slots.insert(other_parent, 5);

        let block = make_valid_block(&consensus, &vrf_kp, proposer, 7, other_parent);
        assert!(
            consensus.validate_block(&block).is_ok(),
            "should accept: parent slot 5 >= locked slot 3 (safe unlock)"
        );
    }

    #[test]
    fn test_validate_block_safe_unlock_accepts_equal_parent_slot() {
        let (mut consensus, vrf_kp, proposer) = create_single_validator_consensus();
        let locked_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 5;
        consensus.current_slot = 10;

        // Parent is at slot 5, EQUAL to locked_slot — should accept (>= threshold)
        let other_parent = H256::from_slice(&[0xEE; 32]).unwrap();
        consensus.block_slots.insert(other_parent, 5);

        let block = make_valid_block(&consensus, &vrf_kp, proposer, 7, other_parent);
        assert!(
            consensus.validate_block(&block).is_ok(),
            "should accept: parent slot == locked slot (equal counts as safe)"
        );
    }

    #[test]
    fn test_mid_epoch_slash_does_not_lower_quorum_threshold() {
        // Setup: 4 validators with 1000 stake each (total 4000).
        // Quorum requires >2/3 of 4000 = 2667 stake.
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, bls2) = create_test_validator_with_bls(1000);
        let (v3, bls3) = create_test_validator_with_bls(1000);
        let (v4, _bls4) = create_test_validator_with_bls(1000);
        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone(), v4.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        // Register BLS keys for first 3 validators
        let addr1 = v1.pubkey.to_address();
        let addr2 = v2.pubkey.to_address();
        let addr3 = v3.pubkey.to_address();
        let pop1 = bls1.proof_of_possession();
        let pop2 = bls2.proof_of_possession();
        let pop3 = bls3.proof_of_possession();
        consensus
            .register_bls_pubkey(addr1, bls1.public_key(), &pop1)
            .unwrap();
        consensus
            .register_bls_pubkey(addr2, bls2.public_key(), &pop2)
            .unwrap();
        consensus
            .register_bls_pubkey(addr3, bls3.public_key(), &pop3)
            .unwrap();

        consensus.advance_slot(); // slot 1, still epoch 0

        // Slash v4 for 100% — live total_stake drops to 3000
        // but epoch_total_stake should remain 4000
        let addr4 = v4.pubkey.to_address();
        consensus.slash_validator(&addr4, 10000);
        assert_eq!(consensus.total_stake, 3000);
        assert_eq!(consensus.epoch_total_stake, 4000);

        // Now try to reach quorum with 2 validators (2000 stake).
        // Against live total (3000), 2000 > 2/3*3000=2000 — would pass.
        // Against epoch total (4000), 2000 < 2/3*4000=2667 — must NOT pass.
        let block_hash = H256::from_slice(&[0xAA; 32]).unwrap();
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&1u64.to_le_bytes());

        let sig1 = bls1.sign(&msg);
        let vote1 = Vote {
            slot: 1,
            block_hash,
            validator: v1.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(sig1),
            stake: 1000,
        };

        let sig2 = bls2.sign(&msg);
        let vote2 = Vote {
            slot: 1,
            block_hash,
            validator: v2.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(sig2),
            stake: 1000,
        };

        let result1 = consensus.process_vote(vote1).unwrap();
        assert!(result1.is_none(), "1 vote should not reach quorum");

        let result2 = consensus.process_vote(vote2).unwrap();
        assert!(
            result2.is_none(),
            "2 votes (2000 stake) should NOT reach quorum against epoch total 4000"
        );

        // Adding a 3rd vote (3000 stake total) should reach quorum (3000 > 2667)
        let sig3 = bls3.sign(&msg);
        let vote3 = Vote {
            slot: 1,
            block_hash,
            validator: v3.pubkey.clone(),
            signature: aether_types::Signature::from_bytes(sig3),
            stake: 1000,
        };
        let result3 = consensus.process_vote(vote3).unwrap();
        assert!(
            result3.is_some(),
            "3 votes (3000 stake) should reach quorum against epoch total 4000"
        );
    }

    #[test]
    fn test_create_vote_uses_epoch_snapshot_stake() {
        // Verify that create_vote uses epoch-frozen stake, not live stake.
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let addr1 = v1.pubkey.to_address();
        let mut consensus =
            HybridConsensus::new(vec![v1.clone()], 0.8, 100, None, Some(bls1), Some(addr1));

        // Slash the validator mid-epoch — live stake drops but epoch stake stays
        consensus.slash_validator(&addr1, 5000); // 50% slash
        assert_eq!(consensus.validators.get(&addr1).unwrap().stake, 500);
        assert_eq!(consensus.epoch_validators.get(&addr1).unwrap().stake, 1000);

        // Vote should use epoch stake (1000), not live stake (500)
        let block_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        let vote = consensus
            .create_vote(block_hash, Phase::Propose)
            .unwrap()
            .unwrap();
        assert_eq!(
            vote.stake, 1000,
            "vote should carry epoch-frozen stake, not live stake"
        );
    }

    #[test]
    fn test_epoch_boundary_snapshots_updated_validators() {
        // After an epoch boundary, the epoch snapshot should reflect slashing.
        let (v1, _bls1) = create_test_validator_with_bls(1000);
        let addr1 = v1.pubkey.to_address();
        let mut consensus = HybridConsensus::new(vec![v1], 0.8, 5, None, None, None);

        // Slash mid-epoch
        consensus.slash_validator(&addr1, 5000); // 50% → 500
        assert_eq!(consensus.epoch_validators.get(&addr1).unwrap().stake, 1000);

        // Advance to epoch boundary (slot 5)
        for _ in 0..5 {
            consensus.advance_slot();
        }

        // Epoch snapshot should now reflect the slashed stake
        assert_eq!(
            consensus.epoch_validators.get(&addr1).unwrap().stake,
            500,
            "epoch snapshot should be updated at epoch boundary"
        );
        assert_eq!(consensus.epoch_total_stake, 500);
    }

    #[test]
    fn test_two_chain_finality_in_propose_phase() {
        // With advance_slot() resetting phase to Propose every slot, only
        // Propose-phase QCs form. Verify that 2-chain finality works:
        // slot 1 QC + slot 2 QC (child of slot 1) → slot 1 finalized.
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, bls2) = create_test_validator_with_bls(1000);
        let (v3, bls3) = create_test_validator_with_bls(1000);

        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        // --- Slot 1: form a Propose QC for block_a ---
        let block_a = H256::from_slice(&[0xAA; 32]).unwrap();
        let parent_a = H256::zero(); // genesis parent
        consensus.current_slot = 1;
        consensus.current_phase = Phase::Propose;
        consensus.block_parents.insert(block_a, parent_a);
        consensus.block_slots.insert(block_a, 1);

        let vote1 = make_signed_vote(&mut consensus, &v1, &bls1, block_a, 1);
        let vote2 = make_signed_vote(&mut consensus, &v2, &bls2, block_a, 1);
        let _vote3 = make_signed_vote(&mut consensus, &v3, &bls3, block_a, 1);

        assert!(consensus.process_vote(vote1).unwrap().is_none());
        assert!(consensus.process_vote(vote2).unwrap().is_some()); // QC at 2/3
                                                                   // Don't need vote3 for quorum; phase advances to Prevote

        assert_eq!(consensus.finalized_slot, 0, "no finality yet — only one QC");

        // --- advance_slot (simulates tick) ---
        consensus.advance_slot();
        assert_eq!(consensus.current_slot, 2);
        assert_eq!(consensus.current_phase, Phase::Propose);

        // --- Slot 2: form a Propose QC for block_b (child of block_a) ---
        let block_b = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.block_parents.insert(block_b, block_a);
        consensus.block_slots.insert(block_b, 2);

        let vote1b = make_signed_vote(&mut consensus, &v1, &bls1, block_b, 2);
        let vote2b = make_signed_vote(&mut consensus, &v2, &bls2, block_b, 2);

        assert!(consensus.process_vote(vote1b).unwrap().is_none());
        let qc = consensus.process_vote(vote2b).unwrap();
        assert!(qc.is_some(), "QC should form for block_b");

        // block_a (slot 1) should now be finalized via 2-chain rule
        assert_eq!(
            consensus.finalized_slot, 1,
            "slot 1 must be finalized: block_a has QC, child block_b has QC"
        );
    }

    #[test]
    fn test_two_chain_finality_no_parent_qc() {
        // If the parent block does NOT have a QC, finality must NOT advance.
        let (v1, bls1) = create_test_validator_with_bls(1000);
        let (v2, bls2) = create_test_validator_with_bls(1000);
        let (v3, _bls3) = create_test_validator_with_bls(1000);

        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );

        // Skip slot 1 (no QC for block_a — e.g. leader failed to propose)
        let block_a = H256::from_slice(&[0xAA; 32]).unwrap();
        consensus.block_parents.insert(block_a, H256::zero());
        consensus.block_slots.insert(block_a, 1);
        // No votes for block_a — no QC

        // --- Slot 2: form QC for block_b (child of block_a) ---
        consensus.current_slot = 2;
        consensus.current_phase = Phase::Propose;
        let block_b = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.block_parents.insert(block_b, block_a);
        consensus.block_slots.insert(block_b, 2);

        let vote1 = make_signed_vote(&mut consensus, &v1, &bls1, block_b, 2);
        let vote2 = make_signed_vote(&mut consensus, &v2, &bls2, block_b, 2);

        consensus.process_vote(vote1).unwrap();
        consensus.process_vote(vote2).unwrap();

        assert_eq!(
            consensus.finalized_slot, 0,
            "no finality — parent block_a has no QC"
        );
    }

    #[test]
    fn test_validate_block_unknown_parent_defaults_to_slot_zero() {
        let (mut consensus, vrf_kp, proposer) = create_single_validator_consensus();
        let locked_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 3;
        consensus.current_slot = 10;

        // Parent hash not in block_slots → defaults to slot 0, which < locked_slot 3
        let unknown_parent = H256::from_slice(&[0xFF; 32]).unwrap();
        let block = make_valid_block(&consensus, &vrf_kp, proposer, 7, unknown_parent);
        assert!(
            consensus.validate_block(&block).is_err(),
            "should reject: unknown parent defaults to slot 0 < locked slot 3"
        );
    }

    #[test]
    fn test_slash_validator_no_overflow_large_stake() {
        // Regression: saturating_mul(slash_bps) overflowed for large stakes,
        // producing u128::MAX instead of the correct slash amount.
        let large_stake = u128::MAX / 2;
        let v = create_test_validator(large_stake);
        let addr = v.pubkey.to_address();
        let mut consensus = HybridConsensus::new(vec![v], 0.8, 100, None, None, None);

        let slashed = consensus.slash_validator(&addr, 500); // 5%
        let expected = large_stake / 20; // exact 5%
                                         // Allow ±1 for rounding
        assert!(
            slashed >= expected - 1 && slashed <= expected + 1,
            "expected ~{expected}, got {slashed}"
        );

        let remaining = consensus.validators.get(&addr).unwrap().stake;
        assert_eq!(remaining, large_stake - slashed);
    }

    #[test]
    fn test_slash_validator_full_range() {
        // Verify slash produces correct result at u128::MAX stake
        let stake = u128::MAX;
        let v = create_test_validator(stake);
        let addr = v.pubkey.to_address();
        let mut consensus = HybridConsensus::new(vec![v], 0.8, 100, None, None, None);

        let slashed = consensus.slash_validator(&addr, 500);
        // Old code: saturating_mul(500) = u128::MAX, then / 10000 = u128::MAX / 10000 ≈ 0.01%
        // Correct: mul_div(u128::MAX, 500, 10000) = 5% of u128::MAX
        let wrong_old_value = u128::MAX / 10000;
        assert!(
            slashed > wrong_old_value,
            "slash {slashed} should be much larger than old wrong value {wrong_old_value}"
        );
    }

    #[test]
    fn epoch_length_zero_does_not_panic() {
        // epoch_length=0 would cause division-by-zero in advance_slot.
        // Constructor must clamp it to >= 1.
        let consensus = HybridConsensus::new(vec![], 0.5, 0, None, None, None);
        // Should not panic — epoch_length internally clamped to 1
        assert_eq!(consensus.epoch_length, 1);
    }

    #[test]
    fn epoch_length_one_transitions_every_slot() {
        let mut consensus = HybridConsensus::new(vec![], 0.5, 1, None, None, None);
        assert_eq!(consensus.current_epoch, 0);
        consensus.advance_slot();
        // epoch_length=1 means epoch transition every slot
        assert_eq!(consensus.current_epoch, 1);
        consensus.advance_slot();
        assert_eq!(consensus.current_epoch, 2);
    }

    /// Byzantine fault tolerance: consensus continues to finalize blocks even when
    /// one of four validators attempts equivocation (double-voting).
    ///
    /// Setup: 4 validators, each 1000 stake → total = 4000.
    /// Quorum threshold: 2/3 of 4000 = 2667 stake.
    ///
    /// The Byzantine validator (v1) casts a legitimate vote for block_a, then
    /// attempts to equivocate by voting for block_evil. The second vote is
    /// rejected. The three honest validators (v2, v3, v4) plus v1's first valid
    /// vote together provide 3000 stake, which exceeds the quorum threshold.
    /// The 2-chain finality rule then finalizes block_a.
    #[test]
    fn test_consensus_tolerates_one_byzantine_equivocator() {
        let (v1, bls1) = create_test_validator_with_bls(1000); // will equivocate
        let (v2, bls2) = create_test_validator_with_bls(1000);
        let (v3, bls3) = create_test_validator_with_bls(1000);
        let (v4, bls4) = create_test_validator_with_bls(1000);

        let mut consensus = HybridConsensus::new(
            vec![v1.clone(), v2.clone(), v3.clone(), v4.clone()],
            0.8,
            100,
            None,
            None,
            None,
        );
        // Total stake = 4000, quorum needs > 2/3 * 4000 = 2667
        assert_eq!(consensus.total_stake(), 4000);

        let block_a = H256::from_slice(&[0xAA; 32]).unwrap();
        let block_evil = H256::from_slice(&[0xEE; 32]).unwrap();
        let parent_a = H256::zero();

        consensus.current_slot = 1;
        consensus.current_phase = Phase::Propose;
        consensus.block_parents.insert(block_a, parent_a);
        consensus.block_slots.insert(block_a, 1);

        // v1 casts a legitimate first vote for block_a (accepted)
        let vote_v1_a = make_signed_vote(&mut consensus, &v1, &bls1, block_a, 1);
        assert!(
            consensus.process_vote(vote_v1_a).is_ok(),
            "v1's first vote (block_a) must be accepted"
        );

        // v1 tries to equivocate: vote for block_evil at the same slot
        let vote_v1_evil = make_signed_vote(&mut consensus, &v1, &bls1, block_evil, 1);
        let equivocation_result = consensus.process_vote(vote_v1_evil);
        assert!(
            equivocation_result.is_err(),
            "Byzantine double-vote must be rejected as equivocation"
        );
        assert!(
            equivocation_result
                .unwrap_err()
                .to_string()
                .contains("equivocation"),
            "Rejection reason must identify equivocation"
        );

        // Honest validators cast votes for block_a.
        // After v2: stake = v1(1000) + v2(1000) = 2000 — still below 2667.
        let vote_v2 = make_signed_vote(&mut consensus, &v2, &bls2, block_a, 1);
        assert!(
            consensus.process_vote(vote_v2).unwrap().is_none(),
            "No QC yet: 2000 stake < 2667 quorum threshold"
        );

        // After v3: stake = 1000 + 1000 + 1000 = 3000 > 2667 → QC forms.
        let vote_v3 = make_signed_vote(&mut consensus, &v3, &bls3, block_a, 1);
        let qc1 = consensus
            .process_vote(vote_v3)
            .expect("v3 vote must not error");
        assert!(
            qc1.is_some(),
            "QC must form: v1+v2+v3 = 3000 stake > 2667 threshold (Byzantine tolerance holds)"
        );

        // One QC is not enough for 2-chain finality
        assert_eq!(
            consensus.finalized_slot, 0,
            "2-chain rule: no finality after one QC"
        );

        // Advance to slot 2 and form a child QC to trigger finality of slot 1.
        consensus.advance_slot();
        assert_eq!(consensus.current_slot, 2);

        let block_b = H256::from_slice(&[0xBB; 32]).unwrap();
        consensus.block_parents.insert(block_b, block_a);
        consensus.block_slots.insert(block_b, 2);

        // Two honest validators suffice for quorum at slot 2 (v3+v4 = 2000, need 2667).
        // Use three to be safe.
        let vote2_v2 = make_signed_vote(&mut consensus, &v2, &bls2, block_b, 2);
        let vote2_v3 = make_signed_vote(&mut consensus, &v3, &bls3, block_b, 2);
        let vote2_v4 = make_signed_vote(&mut consensus, &v4, &bls4, block_b, 2);

        assert!(consensus.process_vote(vote2_v2).unwrap().is_none());
        assert!(consensus.process_vote(vote2_v3).unwrap().is_none()); // v2+v3 = 2000, no QC yet
        let qc2 = consensus
            .process_vote(vote2_v4)
            .expect("v4 vote at slot 2 must not error");
        assert!(
            qc2.is_some(),
            "QC must form at slot 2: v2+v3+v4 = 3000 stake > 2667"
        );

        // 2-chain finality: block_a (slot 1) has QC, block_b (slot 2) has QC as child → finalize.
        assert_eq!(
            consensus.finalized_slot, 1,
            "block_a at slot 1 must be finalized via 2-chain rule despite the Byzantine equivocator"
        );
    }
}
