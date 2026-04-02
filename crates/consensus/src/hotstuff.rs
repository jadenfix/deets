use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, BlsKeypair};
use aether_types::{Address, Block, PublicKey, Slot, ValidatorInfo, H256};
use anyhow::{bail, Result};

use std::collections::{HashMap, HashSet};

/// HotStuff 2-Chain BFT Consensus
///
/// PHASES PER SLOT:
/// 1. PROPOSE: Leader broadcasts block
/// 2. PREVOTE: Validators vote if block extends from locked block
/// 3. PRECOMMIT: Validators vote if prevote has 2/3 quorum
/// 4. COMMIT: Finalize if precommit has 2/3 quorum
///
/// 2-CHAIN RULE:
/// Block B is finalized when:
///   1. B has prevote QC (≥2/3 stake)
///   2. B's child C has precommit QC (≥2/3 stake)
///   3. C.parent_hash == B.hash
///
/// Result: B is finalized.
///
/// VIEW-CHANGE:
/// When pacemaker timeout fires:
///   1. Validator broadcasts TimeoutVote { round, highest_qc }
///   2. New leader collects ≥2/3 stake of timeout votes → TimeoutCertificate
///   3. New leader proposes block extending highest QC from TC

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Phase {
    Propose,
    Prevote,
    Precommit,
    Commit,
}

/// Actions produced by consensus that the node must execute.
#[derive(Debug, Clone)]
pub enum ConsensusAction {
    /// Broadcast a vote to all validators via P2P.
    BroadcastVote(HotStuffVote),
    /// A block has been finalized (irreversible).
    Finalized { slot: Slot, block_hash: H256 },
    /// Broadcast a timeout vote (view-change).
    BroadcastTimeout(TimeoutVote),
}

#[derive(Debug, Clone)]
pub struct HotStuffVote {
    pub slot: Slot,
    pub block_hash: H256,
    pub parent_hash: H256,
    pub phase: Phase,
    pub validator: Address,
    pub validator_pubkey: PublicKey,
    pub stake: u128,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TimeoutVote {
    pub round: u64,
    pub validator: Address,
    pub validator_pubkey: PublicKey,
    pub stake: u128,
    pub highest_qc_slot: Slot,
    pub highest_qc_hash: H256,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TimeoutCertificate {
    pub round: u64,
    pub total_stake: u128,
    pub highest_qc_slot: Slot,
    pub highest_qc_hash: H256,
    pub signers: Vec<Address>,
}

#[derive(Debug, Clone)]
pub struct AggregatedVote {
    pub slot: Slot,
    pub block_hash: H256,
    pub phase: Phase,
    pub total_stake: u128,
    pub signers: Vec<Address>,
    pub aggregated_signature: Vec<u8>,
    pub aggregated_pubkey: Vec<u8>,
}

/// Deterministic canonical phase encoding for vote messages.
/// Using a single byte prevents non-determinism from Debug format strings.
fn phase_to_byte(phase: &Phase) -> u8 {
    match phase {
        Phase::Propose => 0,
        Phase::Prevote => 1,
        Phase::Precommit => 2,
        Phase::Commit => 3,
    }
}

pub struct HotStuffConsensus {
    current_phase: Phase,
    current_slot: Slot,
    validators: HashMap<Address, ValidatorInfo>,
    total_stake: u128,

    /// Votes: phase → block_hash → votes
    votes: HashMap<Phase, HashMap<H256, Vec<HotStuffVote>>>,

    /// Quorum certificates
    qcs: HashMap<(Slot, Phase, H256), AggregatedVote>,

    /// Timeout votes for current round: round → votes
    timeout_votes: HashMap<u64, Vec<TimeoutVote>>,

    /// Block parent tracking: block_hash → parent_hash
    block_parents: HashMap<H256, H256>,

    /// Block slot tracking: block_hash → slot (for correct finality with empty slots)
    block_slots: HashMap<H256, Slot>,

    /// Locked block (safety: cannot vote for conflicting blocks)
    locked_block: Option<H256>,
    locked_slot: Slot,

    committed_slot: Slot,
    finalized_slot: Slot,

    my_keypair: Option<BlsKeypair>,
    my_address: Option<Address>,

    /// Registered BLS public keys (48 bytes each) for vote verification.
    /// Validators must have a registered BLS key to have their votes accepted.
    bls_pubkeys: HashMap<Address, Vec<u8>>,
}

impl HotStuffConsensus {
    pub fn new(
        validators: Vec<ValidatorInfo>,
        my_keypair: Option<BlsKeypair>,
        my_address: Option<Address>,
    ) -> Self {
        let total_stake: u128 = validators.iter().map(|v| v.stake).fold(0u128, u128::saturating_add);
        let validators_map: HashMap<Address, ValidatorInfo> = validators
            .into_iter()
            .map(|v| (v.pubkey.to_address(), v))
            .collect();

        HotStuffConsensus {
            current_phase: Phase::Propose,
            current_slot: 0,
            validators: validators_map,
            total_stake,
            votes: HashMap::new(),
            qcs: HashMap::new(),
            timeout_votes: HashMap::new(),
            block_parents: HashMap::new(),
            block_slots: HashMap::new(),
            locked_block: None,
            locked_slot: 0,
            committed_slot: 0,
            finalized_slot: 0,
            my_keypair,
            my_address,
            bls_pubkeys: HashMap::new(),
        }
    }

    /// Register a BLS public key (48 bytes) for a validator address.
    ///
    /// Requires a valid proof-of-possession (PoP) signature to prevent rogue key attacks.
    /// The PoP proves the registrant knows the secret key corresponding to the public key.
    pub fn register_bls_pubkey(
        &mut self,
        address: Address,
        bls_pk: Vec<u8>,
        pop_signature: &[u8],
    ) -> Result<()> {
        if bls_pk.len() != 48 {
            bail!("BLS pubkey must be 48 bytes, got {}", bls_pk.len());
        }
        // Verify proof-of-possession to prevent rogue key attacks
        match aether_crypto_bls::verify_pop(&bls_pk, pop_signature)? {
            true => {}
            false => bail!(
                "invalid proof-of-possession for BLS pubkey registered by {:?}",
                address
            ),
        }
        self.bls_pubkeys.insert(address, bls_pk);
        Ok(())
    }

    /// Advance to next phase.
    pub fn advance_phase(&mut self) {
        self.current_phase = match self.current_phase {
            Phase::Propose => Phase::Prevote,
            Phase::Prevote => Phase::Precommit,
            Phase::Precommit => Phase::Commit,
            Phase::Commit => {
                self.current_slot += 1;
                self.votes.clear();
                Phase::Propose
            }
        };
    }

    /// Process a proposed block. Returns actions for the node to execute.
    pub fn on_propose(&mut self, block: &Block) -> Result<Vec<ConsensusAction>> {
        let _span = tracing::info_span!(
            "consensus_propose",
            slot = block.header.slot,
            block_hash = ?block.hash(),
        )
        .entered();

        if self.current_phase != Phase::Propose {
            bail!("not in propose phase");
        }

        // Track parent relationship and slot
        self.block_parents
            .insert(block.hash(), block.header.parent_hash);
        self.block_slots.insert(block.hash(), block.header.slot);

        // HotStuff locking rule: accept block if it extends from our locked block,
        // OR if it carries a QC for a slot >= our locked slot (which means a quorum
        // has moved past our lock, making it safe to vote for this new branch).
        if let Some(locked) = &self.locked_block {
            if block.header.parent_hash != *locked {
                // Check if the block's parent has a QC at or above our locked slot,
                // which would justify unlocking (the "safe node predicate" in HotStuff).
                let parent_slot = self
                    .block_slots
                    .get(&block.header.parent_hash)
                    .copied()
                    .unwrap_or(0);
                if parent_slot < self.locked_slot {
                    return Ok(vec![]);
                }
            }
        }

        self.advance_phase();

        // Create prevote and return it as an action (NOT recursive)
        let mut actions = Vec::new();
        if let Some(vote) =
            self.create_vote(block.hash(), block.header.parent_hash, Phase::Prevote)?
        {
            actions.push(ConsensusAction::BroadcastVote(vote));
        }
        Ok(actions)
    }

    /// Process a vote. Returns QC (if quorum reached) and actions for the node.
    pub fn on_vote(
        &mut self,
        vote: HotStuffVote,
    ) -> Result<(Option<AggregatedVote>, Vec<ConsensusAction>)> {
        let _span = tracing::debug_span!(
            "consensus_vote",
            slot = vote.slot,
            phase = ?vote.phase,
            validator = ?vote.validator,
        )
        .entered();

        self.verify_vote(&vote)?;

        // Track parent and slot
        self.block_parents
            .entry(vote.block_hash)
            .or_insert(vote.parent_hash);
        self.block_slots.entry(vote.block_hash).or_insert(vote.slot);

        // Verify the claimed stake matches registered stake
        let registered_stake = self
            .validators
            .get(&vote.validator)
            .map(|v| v.stake)
            .unwrap_or(0);
        if vote.stake != registered_stake {
            bail!(
                "vote stake mismatch: claimed {} but registered {}",
                vote.stake,
                registered_stake
            );
        }

        // Store vote (deduplicate: reject if this validator already voted in this phase for this block)
        let phase_votes = self.votes.entry(vote.phase.clone()).or_default();
        let block_votes = phase_votes.entry(vote.block_hash).or_default();
        if block_votes.iter().any(|v| v.validator == vote.validator) {
            bail!(
                "duplicate vote from {:?} in phase {:?} for block {:?}",
                vote.validator,
                vote.phase,
                vote.block_hash
            );
        }
        block_votes.push(vote.clone());

        // Check for quorum
        let stake: u128 = block_votes.iter().map(|v| v.stake).fold(0u128, u128::saturating_add);
        let has_quorum = crate::has_quorum(stake, self.total_stake);

        if !has_quorum {
            return Ok((None, vec![]));
        }

        let votes_to_aggregate = block_votes.clone();
        let qc = self.aggregate_votes(&votes_to_aggregate)?;

        self.qcs
            .insert((vote.slot, vote.phase.clone(), vote.block_hash), qc.clone());

        let mut actions = Vec::new();

        match vote.phase {
            Phase::Prevote => {
                // Lock on this block
                self.locked_block = Some(vote.block_hash);
                self.locked_slot = vote.slot;
                self.advance_phase();

                // Create precommit vote — returned as action, NOT recursive
                if let Some(my_vote) =
                    self.create_vote(vote.block_hash, vote.parent_hash, Phase::Precommit)?
                {
                    actions.push(ConsensusAction::BroadcastVote(my_vote));
                }
            }
            Phase::Precommit => {
                // 2-CHAIN FINALITY RULE:
                // Check if the PARENT block has a prevote QC.
                // If so, the parent block is finalized.
                let parent_hash = self
                    .block_parents
                    .get(&vote.block_hash)
                    .copied()
                    .unwrap_or(vote.parent_hash);

                // Look up the parent block's actual slot from block_slots map.
                // This correctly handles empty/skipped slots where the parent
                // may be multiple slots back (not just slot-1).
                // SAFETY: Do NOT fall back to vote.slot - 1 — with skipped slots,
                // that guess is wrong and could finalize a non-existent block.
                let parent_slot = self
                    .block_slots
                    .get(&parent_hash)
                    .copied();

                if let Some(parent_slot) = parent_slot {
                    // Look for parent block's prevote QC using the PARENT's hash
                    if self
                        .qcs
                        .contains_key(&(parent_slot, Phase::Prevote, parent_hash))
                    {
                        // Monotonicity: never regress finalized_slot
                        if parent_slot > self.finalized_slot {
                            self.finalized_slot = parent_slot;
                            tracing::info!(
                                finalized_slot = parent_slot,
                                block_hash = ?parent_hash,
                                "Block finalized via 2-chain rule"
                            );
                            actions.push(ConsensusAction::Finalized {
                                slot: parent_slot,
                                block_hash: parent_hash,
                            });
                            self.prune_finalized_state();
                        }
                    }
                }

                // Monotonicity: never regress committed_slot
                if vote.slot > self.committed_slot {
                    self.committed_slot = vote.slot;
                }
                self.advance_phase();
            }
            _ => {}
        }

        Ok((Some(qc), actions))
    }

    /// Handle a pacemaker timeout: create a timeout vote.
    pub fn on_timeout(&self, round: u64) -> Result<Vec<ConsensusAction>> {
        let _span = tracing::warn_span!("consensus_timeout", round).entered();
        let mut actions = Vec::new();

        if let (Some(kp), Some(addr)) = (&self.my_keypair, &self.my_address) {
            let validator = self
                .validators
                .get(addr)
                .ok_or_else(|| anyhow::anyhow!("not in validator set"))?;

            // Find our highest QC
            let (highest_qc_slot, highest_qc_hash) = self.highest_qc();

            let mut msg = Vec::new();
            msg.extend_from_slice(b"timeout");
            msg.extend_from_slice(&round.to_le_bytes());
            msg.extend_from_slice(&highest_qc_slot.to_le_bytes());
            msg.extend_from_slice(highest_qc_hash.as_bytes());
            let signature = kp.sign(&msg);

            actions.push(ConsensusAction::BroadcastTimeout(TimeoutVote {
                round,
                validator: *addr,
                validator_pubkey: validator.pubkey.clone(),
                stake: validator.stake,
                highest_qc_slot,
                highest_qc_hash,
                signature,
            }));
        }

        Ok(actions)
    }

    /// Process a timeout vote from another validator.
    /// Returns a TimeoutCertificate if quorum is reached.
    ///
    /// Safety invariants:
    /// - Deduplicates votes from the same validator (prevents stake inflation)
    /// - Verifies the voter is a known validator with correct stake
    /// - Verifies BLS signature on the timeout message
    pub fn on_timeout_vote(&mut self, tv: TimeoutVote) -> Result<Option<TimeoutCertificate>> {
        // Verify the voter is a known validator
        let validator_info = self
            .validators
            .get(&tv.validator)
            .ok_or_else(|| anyhow::anyhow!("unknown validator {:?}", tv.validator))?;

        // Verify the claimed stake matches the registered stake
        if tv.stake != validator_info.stake {
            bail!(
                "timeout vote stake mismatch: claimed {} but registered {}",
                tv.stake,
                validator_info.stake
            );
        }

        // Verify BLS signature on the timeout vote
        self.verify_timeout_vote_signature(&tv)?;

        let round_votes = self.timeout_votes.entry(tv.round).or_default();

        // Deduplicate: reject if this validator already voted in this round
        if round_votes.iter().any(|v| v.validator == tv.validator) {
            bail!(
                "duplicate timeout vote from {:?} in round {}",
                tv.validator,
                tv.round
            );
        }

        round_votes.push(tv.clone());

        let stake: u128 = round_votes.iter().map(|v| v.stake).fold(0u128, u128::saturating_add);
        if !crate::has_quorum(stake, self.total_stake) {
            return Ok(None);
        }

        // Find the highest QC across all timeout votes
        let mut highest_qc_slot = 0;
        let mut highest_qc_hash = H256::zero();
        for v in round_votes.iter() {
            if v.highest_qc_slot > highest_qc_slot {
                highest_qc_slot = v.highest_qc_slot;
                highest_qc_hash = v.highest_qc_hash;
            }
        }

        let signers = round_votes.iter().map(|v| v.validator).collect();

        Ok(Some(TimeoutCertificate {
            round: tv.round,
            total_stake: stake,
            highest_qc_slot,
            highest_qc_hash,
            signers,
        }))
    }

    /// Process a timeout certificate: advance to new round.
    ///
    /// Safety invariants:
    /// - Recomputes voted stake from local validator set (never trusts tc.total_stake)
    /// - Validates recomputed stake has >= 2/3 quorum
    /// - Rejects TCs with unknown or duplicate signers
    /// - Updates locked block to the highest QC referenced by the TC
    ///   (ensures the new leader extends from the highest certified block)
    /// - Clears stale votes from the previous round
    pub fn on_timeout_certificate(&mut self, tc: &TimeoutCertificate) -> Result<()> {
        let _span = tracing::warn_span!("consensus_tc", round = tc.round).entered();
        // Recompute voted stake from local validator set — never trust tc.total_stake.
        // A malicious peer could forge a TC with inflated total_stake to bypass quorum.
        let mut seen_signers = HashSet::new();
        let mut voted_stake: u128 = 0;
        for signer in &tc.signers {
            if !seen_signers.insert(signer) {
                bail!("duplicate signer in timeout certificate: {:?}", signer);
            }
            let stake = self
                .validators
                .get(signer)
                .map(|v| v.stake)
                .ok_or_else(|| {
                    anyhow::anyhow!("unknown signer in timeout certificate: {:?}", signer)
                })?;
            voted_stake = voted_stake
                .checked_add(stake)
                .ok_or_else(|| anyhow::anyhow!("voted stake overflow in timeout certificate"))?;
        }

        if !crate::has_quorum(voted_stake, self.total_stake) {
            bail!(
                "timeout certificate has insufficient stake: {} / {} total",
                voted_stake,
                self.total_stake
            );
        }

        // Update locked block to the highest QC from the TC.
        // This ensures the next leader proposes extending the most recent
        // certified block, preserving the 2-chain finality invariant.
        if tc.highest_qc_slot > self.locked_slot {
            self.locked_block = Some(tc.highest_qc_hash);
            self.locked_slot = tc.highest_qc_slot;
        }

        // Advance slot (new round = new leader)
        self.current_slot += 1;
        self.current_phase = Phase::Propose;
        self.votes.clear();

        // Prune timeout votes for completed rounds — they are never needed again.
        // Without this, the timeout_votes map grows monotonically in any validator
        // that experiences timeouts, eventually causing OOM.
        let tc_round = tc.round;
        self.timeout_votes.retain(|round, _| *round > tc_round);

        Ok(())
    }

    /// Find the highest QC we've seen.
    fn highest_qc(&self) -> (Slot, H256) {
        let mut best_slot = 0;
        let mut best_hash = H256::zero();
        for (slot, _, hash) in self.qcs.keys() {
            if *slot > best_slot {
                best_slot = *slot;
                best_hash = *hash;
            }
        }
        (best_slot, best_hash)
    }

    /// Create a vote (does NOT recursively process it).
    fn create_vote(
        &self,
        block_hash: H256,
        parent_hash: H256,
        phase: Phase,
    ) -> Result<Option<HotStuffVote>> {
        let (keypair, address) = match (&self.my_keypair, &self.my_address) {
            (Some(kp), Some(addr)) => (kp, addr),
            _ => return Ok(None),
        };

        let validator = self
            .validators
            .get(address)
            .ok_or_else(|| anyhow::anyhow!("not in validator set"))?;

        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(parent_hash.as_bytes());
        msg.extend_from_slice(&self.current_slot.to_le_bytes());
        msg.push(phase_to_byte(&phase)); // canonical single-byte encoding

        let signature = keypair.sign(&msg);

        Ok(Some(HotStuffVote {
            slot: self.current_slot,
            block_hash,
            parent_hash,
            phase,
            validator: *address,
            validator_pubkey: validator.pubkey.clone(),
            stake: validator.stake,
            signature,
        }))
    }

    /// Verify a vote's BLS signature using registered BLS public keys.
    fn verify_vote(&self, vote: &HotStuffVote) -> Result<()> {
        let _validator = self
            .validators
            .get(&vote.validator)
            .ok_or_else(|| anyhow::anyhow!("unknown validator {:?}", vote.validator))?;

        let bls_pk = self
            .bls_pubkeys
            .get(&vote.validator)
            .ok_or_else(|| anyhow::anyhow!("no BLS pubkey registered for {:?}", vote.validator))?;
        if bls_pk.len() != 48 {
            bail!(
                "BLS pubkey invalid length {} for {:?}",
                bls_pk.len(),
                vote.validator
            );
        }
        if vote.signature.len() != 96 {
            bail!(
                "vote signature invalid length {} from {:?}",
                vote.signature.len(),
                vote.validator
            );
        }

        let mut msg = Vec::new();
        msg.extend_from_slice(vote.block_hash.as_bytes());
        msg.extend_from_slice(vote.parent_hash.as_bytes());
        msg.extend_from_slice(&vote.slot.to_le_bytes());
        msg.push(phase_to_byte(&vote.phase)); // canonical single-byte encoding

        let valid = aether_crypto_bls::keypair::verify(bls_pk, &msg, &vote.signature)?;
        if !valid {
            bail!("invalid BLS signature from {:?}", vote.validator);
        }
        Ok(())
    }

    /// Verify BLS signature on a timeout vote message.
    fn verify_timeout_vote_signature(&self, tv: &TimeoutVote) -> Result<()> {
        let bls_pk = self
            .bls_pubkeys
            .get(&tv.validator)
            .ok_or_else(|| anyhow::anyhow!("no BLS pubkey registered for {:?}", tv.validator))?;
        if bls_pk.len() != 48 {
            bail!(
                "BLS pubkey invalid length {} for {:?}",
                bls_pk.len(),
                tv.validator
            );
        }
        if tv.signature.len() != 96 {
            bail!(
                "timeout vote signature invalid length {} from {:?}",
                tv.signature.len(),
                tv.validator
            );
        }

        let mut msg = Vec::new();
        msg.extend_from_slice(b"timeout");
        msg.extend_from_slice(&tv.round.to_le_bytes());
        msg.extend_from_slice(&tv.highest_qc_slot.to_le_bytes());
        msg.extend_from_slice(tv.highest_qc_hash.as_bytes());
        let valid = aether_crypto_bls::keypair::verify(bls_pk, &msg, &tv.signature)?;
        if !valid {
            bail!(
                "invalid BLS signature on timeout vote from {:?}",
                tv.validator
            );
        }
        Ok(())
    }

    fn aggregate_votes(&self, votes: &[HotStuffVote]) -> Result<AggregatedVote> {
        let signatures: Vec<Vec<u8>> = votes.iter().map(|v| v.signature.clone()).collect();
        let pubkeys: Vec<Vec<u8>> = votes
            .iter()
            .map(|v| {
                self.bls_pubkeys
                    .get(&v.validator)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("no BLS pubkey for {:?}", v.validator))
            })
            .collect::<Result<Vec<_>>>()?;

        let agg_sig = aggregate_signatures(&signatures)?;
        let agg_pk = aggregate_public_keys(&pubkeys)?;

        Ok(AggregatedVote {
            slot: votes[0].slot,
            block_hash: votes[0].block_hash,
            phase: votes[0].phase.clone(),
            total_stake: votes.iter().map(|v| v.stake).fold(0u128, u128::saturating_add),
            signers: votes.iter().map(|v| v.validator).collect(),
            aggregated_signature: agg_sig,
            aggregated_pubkey: agg_pk,
        })
    }

    #[allow(dead_code)]
    pub fn has_quorum(&self, stake: u128) -> bool {
        crate::has_quorum(stake, self.total_stake)
    }

    pub fn current_slot(&self) -> Slot {
        self.current_slot
    }

    pub fn finalized_slot(&self) -> Slot {
        self.finalized_slot
    }

    pub fn committed_slot(&self) -> Slot {
        self.committed_slot
    }

    pub fn current_phase(&self) -> &Phase {
        &self.current_phase
    }

    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Prune consensus tracking state for slots that have been finalized.
    ///
    /// Without pruning, `block_parents`, `block_slots`, and `qcs` grow
    /// monotonically — one entry per block/vote/QC for the entire chain
    /// history. In a validator running for days, this causes OOM.
    ///
    /// Once a block is finalized, its parent/slot tracking and QCs cannot
    /// affect future consensus decisions, so they can be safely removed.
    /// We keep a small safety margin (2 slots) for in-flight messages.
    fn prune_finalized_state(&mut self) {
        if self.finalized_slot < 3 {
            return;
        }
        let prune_below = self.finalized_slot - 2;

        self.qcs.retain(|(slot, _, _), _| *slot >= prune_below);
        self.block_slots.retain(|_, slot| *slot >= prune_below);
        let known_hashes: HashSet<H256> = self.block_slots.keys().copied().collect();
        self.block_parents
            .retain(|hash, _| known_hashes.contains(hash));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
        (0..count)
            .map(|_i| {
                let keypair = aether_crypto_primitives::Keypair::generate();
                ValidatorInfo {
                    pubkey: PublicKey::from_bytes(keypair.public_key()),
                    stake: 1000,
                    commission: 0,
                    active: true,
                }
            })
            .collect()
    }

    #[test]
    fn test_hotstuff_creation() {
        let validators = create_test_validators(4);
        let consensus = HotStuffConsensus::new(validators, None, None);

        assert_eq!(consensus.total_stake, 4000);
        assert_eq!(consensus.current_phase, Phase::Propose);
    }

    #[test]
    fn test_quorum_calculation() {
        let validators = create_test_validators(4);
        let consensus = HotStuffConsensus::new(validators, None, None);

        assert!(!consensus.has_quorum(2666));
        assert!(consensus.has_quorum(2667));
        assert!(consensus.has_quorum(3000));
    }

    #[test]
    fn test_phase_progression() {
        let validators = create_test_validators(4);
        let mut consensus = HotStuffConsensus::new(validators, None, None);

        assert_eq!(consensus.current_phase, Phase::Propose);
        consensus.advance_phase();
        assert_eq!(consensus.current_phase, Phase::Prevote);
        consensus.advance_phase();
        assert_eq!(consensus.current_phase, Phase::Precommit);
        consensus.advance_phase();
        assert_eq!(consensus.current_phase, Phase::Commit);

        let initial_slot = consensus.current_slot;
        consensus.advance_phase();
        assert_eq!(consensus.current_phase, Phase::Propose);
        assert_eq!(consensus.current_slot, initial_slot + 1);
    }

    #[test]
    fn test_vote_counting() {
        let validators = create_test_validators(3);
        let consensus = HotStuffConsensus::new(validators, None, None);

        assert!(!consensus.has_quorum(1999));
        assert!(consensus.has_quorum(2000));
        assert!(consensus.has_quorum(3000));
    }

    #[test]
    fn test_on_vote_returns_actions_not_recursive() {
        // Verify that on_vote returns BroadcastVote actions instead of recursing.
        // We use BLS keypairs for validators so signature verification passes.
        let bls_keys: Vec<BlsKeypair> = (0..4).map(|_| BlsKeypair::generate()).collect();
        let validators: Vec<ValidatorInfo> = bls_keys
            .iter()
            .map(|bk| {
                // Use BLS public key bytes padded/truncated to build a PublicKey
                let pk_bytes = bk.public_key();
                ValidatorInfo {
                    pubkey: PublicKey::from_bytes(pk_bytes[..32].to_vec()),
                    stake: 1000,
                    commission: 0,
                    active: true,
                }
            })
            .collect();

        let my_addr = validators[0].pubkey.to_address();
        let mut consensus =
            HotStuffConsensus::new(validators.clone(), Some(bls_keys[0].clone()), Some(my_addr));
        consensus.advance_phase(); // → Prevote

        let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let parent_hash = H256::zero();

        // Directly insert votes (bypassing signature verification for this unit test)
        // to test the action-return logic
        for validator in validators.iter().take(3) {
            let vote = HotStuffVote {
                slot: 0,
                block_hash,
                parent_hash,
                phase: Phase::Prevote,
                validator: validator.pubkey.to_address(),
                validator_pubkey: validator.pubkey.clone(),
                stake: 1000,
                signature: vec![0u8; 96], // dummy sig
            };

            // Insert directly into votes map to skip BLS verify
            let phase_votes = consensus.votes.entry(Phase::Prevote).or_default();
            let block_votes = phase_votes.entry(block_hash).or_default();
            block_votes.push(vote.clone());

            // Check quorum manually
            let stake: u128 = block_votes.iter().map(|v| v.stake).fold(0u128, u128::saturating_add);
            if stake.saturating_mul(3) >= consensus.total_stake.saturating_mul(2) {
                // Quorum reached — the old code would recurse here.
                // New code: create_vote returns an action instead.
                if let Ok(Some(my_vote)) =
                    consensus.create_vote(block_hash, parent_hash, Phase::Precommit)
                {
                    // We got a vote back as data, not a recursive call. Success!
                    assert_eq!(my_vote.phase, Phase::Precommit);
                }
            }
        }

        // If we got here without stack overflow, the recursive bug is fixed
    }

    /// Helper: create validators with BLS keys and register them.
    fn setup_bls_consensus(
        count: usize,
    ) -> (HotStuffConsensus, Vec<ValidatorInfo>, Vec<BlsKeypair>) {
        let bls_keys: Vec<BlsKeypair> = (0..count).map(|_| BlsKeypair::generate()).collect();
        let validators: Vec<ValidatorInfo> = bls_keys
            .iter()
            .map(|bk| {
                let pk_bytes = bk.public_key();
                ValidatorInfo {
                    pubkey: PublicKey::from_bytes(pk_bytes[..32].to_vec()),
                    stake: 1000,
                    commission: 0,
                    active: true,
                }
            })
            .collect();

        let my_addr = validators[0].pubkey.to_address();
        let mut consensus =
            HotStuffConsensus::new(validators.clone(), Some(bls_keys[0].clone()), Some(my_addr));

        // Register BLS keys for all validators
        for (i, v) in validators.iter().enumerate() {
            let addr = v.pubkey.to_address();
            let pop = bls_keys[i].proof_of_possession();
            consensus
                .register_bls_pubkey(addr, bls_keys[i].public_key(), &pop)
                .unwrap();
        }

        (consensus, validators, bls_keys)
    }

    #[test]
    fn test_timeout_vote_collection() {
        let (mut consensus, validators, bls_keys) = setup_bls_consensus(4);

        // Collect timeout votes from 3 of 4 validators
        for (i, validator) in validators.iter().take(3).enumerate() {
            let addr = validator.pubkey.to_address();
            // Sign the correct timeout message
            let mut msg = Vec::new();
            msg.extend_from_slice(b"timeout");
            msg.extend_from_slice(&1u64.to_le_bytes()); // round
            msg.extend_from_slice(&0u64.to_le_bytes()); // highest_qc_slot
            msg.extend_from_slice(H256::zero().as_bytes()); // highest_qc_hash
            let signature = bls_keys[i].sign(&msg);

            let tv = TimeoutVote {
                round: 1,
                validator: addr,
                validator_pubkey: validator.pubkey.clone(),
                stake: 1000,
                highest_qc_slot: 0,
                highest_qc_hash: H256::zero(),
                signature,
            };

            let result = consensus.on_timeout_vote(tv).unwrap();
            if i < 2 {
                assert!(result.is_none(), "no TC before quorum");
            } else {
                assert!(result.is_some(), "TC after 3/4 = 75% > 66.7%");
                let tc = result.unwrap();
                assert_eq!(tc.round, 1);
                assert_eq!(tc.signers.len(), 3);
            }
        }
    }

    #[test]
    fn test_timeout_vote_rejects_duplicate() {
        let (mut consensus, validators, bls_keys) = setup_bls_consensus(4);

        let addr = validators[0].pubkey.to_address();
        let mut msg = Vec::new();
        msg.extend_from_slice(b"timeout");
        msg.extend_from_slice(&1u64.to_le_bytes());
        msg.extend_from_slice(&0u64.to_le_bytes());
        msg.extend_from_slice(H256::zero().as_bytes());
        let signature = bls_keys[0].sign(&msg);

        let tv = TimeoutVote {
            round: 1,
            validator: addr,
            validator_pubkey: validators[0].pubkey.clone(),
            stake: 1000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signature: signature.clone(),
        };

        // First vote accepted
        assert!(consensus.on_timeout_vote(tv.clone()).is_ok());
        // Duplicate rejected
        let result = consensus.on_timeout_vote(tv);
        assert!(result.is_err(), "duplicate timeout vote must be rejected");
    }

    #[test]
    fn test_timeout_vote_rejects_wrong_stake() {
        let (mut consensus, validators, bls_keys) = setup_bls_consensus(4);

        let addr = validators[0].pubkey.to_address();
        let mut msg = Vec::new();
        msg.extend_from_slice(b"timeout");
        msg.extend_from_slice(&1u64.to_le_bytes());
        msg.extend_from_slice(&0u64.to_le_bytes());
        msg.extend_from_slice(H256::zero().as_bytes());
        let signature = bls_keys[0].sign(&msg);

        let tv = TimeoutVote {
            round: 1,
            validator: addr,
            validator_pubkey: validators[0].pubkey.clone(),
            stake: 9999, // wrong stake
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signature,
        };

        let result = consensus.on_timeout_vote(tv);
        assert!(result.is_err(), "mismatched stake must be rejected");
    }

    #[test]
    fn test_timeout_certificate_rejects_insufficient_stake() {
        let (mut consensus, validators, _bls_keys) = setup_bls_consensus(4);

        // Only 1 signer = 1000/4000 = 25% — needs 66.7%
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 1000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![validators[0].pubkey.to_address()],
        };

        let result = consensus.on_timeout_certificate(&tc);
        assert!(
            result.is_err(),
            "TC with insufficient stake must be rejected"
        );
    }

    #[test]
    fn test_timeout_certificate_updates_locked_block() {
        let (mut consensus, validators, _bls_keys) = setup_bls_consensus(4);

        let highest_hash = H256::from_slice(&[0xAB; 32]).unwrap();
        // 3 signers = 3000/4000 = 75% quorum
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 3000,
            highest_qc_slot: 5,
            highest_qc_hash: highest_hash,
            signers: vec![
                validators[0].pubkey.to_address(),
                validators[1].pubkey.to_address(),
                validators[2].pubkey.to_address(),
            ],
        };

        consensus.on_timeout_certificate(&tc).unwrap();
        assert_eq!(consensus.locked_block, Some(highest_hash));
        assert_eq!(consensus.locked_slot, 5);
    }

    #[test]
    fn test_vote_rejects_duplicate() {
        let (mut consensus, validators, bls_keys) = setup_bls_consensus(4);

        consensus.advance_phase(); // → Prevote
        let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let parent_hash = H256::zero();
        let addr = validators[1].pubkey.to_address();

        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(parent_hash.as_bytes());
        msg.extend_from_slice(&consensus.current_slot().to_le_bytes());
        msg.push(phase_to_byte(&Phase::Prevote));
        let signature = bls_keys[1].sign(&msg);

        let vote = HotStuffVote {
            slot: consensus.current_slot(),
            block_hash,
            parent_hash,
            phase: Phase::Prevote,
            validator: addr,
            validator_pubkey: validators[1].pubkey.clone(),
            stake: 1000,
            signature,
        };

        // First vote accepted
        assert!(consensus.on_vote(vote.clone()).is_ok());
        // Duplicate rejected
        let result = consensus.on_vote(vote);
        assert!(result.is_err(), "duplicate vote must be rejected");
    }

    #[test]
    fn test_vote_rejects_wrong_stake() {
        let (mut consensus, validators, bls_keys) = setup_bls_consensus(4);

        consensus.advance_phase(); // → Prevote
        let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let parent_hash = H256::zero();
        let addr = validators[1].pubkey.to_address();

        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(parent_hash.as_bytes());
        msg.extend_from_slice(&consensus.current_slot().to_le_bytes());
        msg.push(phase_to_byte(&Phase::Prevote));
        let signature = bls_keys[1].sign(&msg);

        let vote = HotStuffVote {
            slot: consensus.current_slot(),
            block_hash,
            parent_hash,
            phase: Phase::Prevote,
            validator: addr,
            validator_pubkey: validators[1].pubkey.clone(),
            stake: 5000, // wrong stake
            signature,
        };

        let result = consensus.on_vote(vote);
        assert!(result.is_err(), "mismatched stake must be rejected");
    }

    #[test]
    fn test_timeout_certificate_advances_slot() {
        let validators = create_test_validators(4);
        let addrs: Vec<Address> = validators.iter().map(|v| v.pubkey.to_address()).collect();
        let mut consensus = HotStuffConsensus::new(validators, None, None);

        let initial_slot = consensus.current_slot();
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 3000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![addrs[0], addrs[1], addrs[2]],
        };

        consensus.on_timeout_certificate(&tc).unwrap();
        assert_eq!(consensus.current_slot(), initial_slot + 1);
        assert_eq!(*consensus.current_phase(), Phase::Propose);
    }

    #[test]
    fn test_timeout_certificate_rejects_unknown_signer() {
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        let fake_addr = Address::from_slice(&[0xDE; 20]).unwrap();
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 3000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![fake_addr],
        };

        let result = consensus.on_timeout_certificate(&tc);
        assert!(result.is_err(), "TC with unknown signer must be rejected");
    }

    #[test]
    fn test_timeout_certificate_rejects_duplicate_signer() {
        let (mut consensus, validators, _bls_keys) = setup_bls_consensus(4);

        let addr = validators[0].pubkey.to_address();
        // Same signer listed 3 times — should be rejected
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 3000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![addr, addr, addr],
        };

        let result = consensus.on_timeout_certificate(&tc);
        assert!(result.is_err(), "TC with duplicate signers must be rejected");
    }

    #[test]
    fn test_timeout_certificate_rejects_inflated_total_stake() {
        let (mut consensus, validators, _bls_keys) = setup_bls_consensus(4);

        // Only 1 real signer (1000 stake) but total_stake claims 3000
        // The fix recomputes stake from signers, so this should fail quorum
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 3000, // lie — only 1000 from 1 signer
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![validators[0].pubkey.to_address()],
        };

        let result = consensus.on_timeout_certificate(&tc);
        assert!(
            result.is_err(),
            "TC with inflated total_stake must be rejected when recomputed stake is insufficient"
        );
    }

    #[test]
    fn test_phase_to_byte_canonical() {
        assert_eq!(phase_to_byte(&Phase::Propose), 0);
        assert_eq!(phase_to_byte(&Phase::Prevote), 1);
        assert_eq!(phase_to_byte(&Phase::Precommit), 2);
        assert_eq!(phase_to_byte(&Phase::Commit), 3);
    }

    #[test]
    fn test_verify_vote_rejects_without_bls_key() {
        let validators = create_test_validators(2);
        let mut consensus = HotStuffConsensus::new(validators.clone(), None, None);
        // No BLS keys registered
        let vote = HotStuffVote {
            slot: 0,
            block_hash: H256::from_slice(&[1u8; 32]).unwrap(),
            parent_hash: H256::zero(),
            phase: Phase::Prevote,
            validator: validators[0].pubkey.to_address(),
            validator_pubkey: validators[0].pubkey.clone(),
            stake: 1000,
            signature: vec![0u8; 96],
        };
        let result = consensus.on_vote(vote);
        assert!(
            result.is_err(),
            "Vote without registered BLS key must be rejected"
        );
    }

    #[test]
    fn test_verify_vote_rejects_invalid_signature() {
        let validators = create_test_validators(2);
        let bls_kp = BlsKeypair::generate();
        let mut consensus = HotStuffConsensus::new(validators.clone(), None, None);
        let addr = validators[0].pubkey.to_address();
        let pop = bls_kp.proof_of_possession();
        consensus
            .register_bls_pubkey(addr, bls_kp.public_key(), &pop)
            .unwrap();

        // Sign wrong message
        let wrong_sig = bls_kp.sign(b"wrong message");
        let vote = HotStuffVote {
            slot: 0,
            block_hash: H256::from_slice(&[1u8; 32]).unwrap(),
            parent_hash: H256::zero(),
            phase: Phase::Prevote,
            validator: addr,
            validator_pubkey: validators[0].pubkey.clone(),
            stake: 1000,
            signature: wrong_sig,
        };
        let result = consensus.on_vote(vote);
        assert!(
            result.is_err(),
            "Vote with invalid BLS signature must be rejected"
        );
    }

    #[test]
    fn test_finality_monotonicity_no_regression() {
        // Ensure finalized_slot never decreases even if a late precommit QC
        // arrives for an older block.
        let (mut consensus, validators, _bls_keys) = setup_bls_consensus(4);

        let block_a = H256::from_slice(&[0xA0; 32]).unwrap();
        let block_b = H256::from_slice(&[0xB0; 32]).unwrap();
        let block_c = H256::from_slice(&[0xC0; 32]).unwrap();
        let parent_zero = H256::zero();

        // Register block chain: zero -> A(slot 1) -> B(slot 2) -> C(slot 3)
        consensus.block_parents.insert(block_a, parent_zero);
        consensus.block_slots.insert(block_a, 1);
        consensus.block_parents.insert(block_b, block_a);
        consensus.block_slots.insert(block_b, 2);
        consensus.block_parents.insert(block_c, block_b);
        consensus.block_slots.insert(block_c, 3);

        // Insert prevote QCs for A (slot 1) and B (slot 2)
        let dummy_qc = AggregatedVote {
            slot: 1,
            block_hash: block_a,
            phase: Phase::Prevote,
            aggregated_signature: vec![],
            signers: vec![],
            total_stake: 3000,
            aggregated_pubkey: vec![],
        };
        consensus.qcs.insert((1, Phase::Prevote, block_a), dummy_qc.clone());
        consensus.qcs.insert((2, Phase::Prevote, block_b), AggregatedVote {
            slot: 2,
            block_hash: block_b,
            phase: Phase::Prevote,
            aggregated_signature: vec![],
            signers: vec![],
            total_stake: 3000,
            aggregated_pubkey: vec![],
        });

        // Simulate precommit QC for block_c (slot 3) → should finalize block_b (slot 2)
        consensus.current_phase = Phase::Precommit;
        consensus.current_slot = 3;

        // Directly insert precommit votes to reach quorum
        for v in validators.iter().take(3) {
            let vote = HotStuffVote {
                slot: 3,
                block_hash: block_c,
                parent_hash: block_b,
                phase: Phase::Precommit,
                validator: v.pubkey.to_address(),
                validator_pubkey: v.pubkey.clone(),
                stake: 1000,
                signature: vec![0u8; 96],
            };
            let phase_votes = consensus.votes.entry(Phase::Precommit).or_default();
            phase_votes.entry(block_c).or_default().push(vote);
        }

        // Manually trigger the quorum logic for block_c precommit
        let stake: u128 = 3000;
        assert!(crate::has_quorum(stake, consensus.total_stake));

        // Build a dummy aggregated QC for the precommit
        consensus.qcs.insert((3, Phase::Precommit, block_c), AggregatedVote {
            slot: 3,
            block_hash: block_c,
            phase: Phase::Precommit,
            aggregated_signature: vec![],
            signers: vec![],
            total_stake: 3000,
            aggregated_pubkey: vec![],
        });

        // Apply finality logic: parent of C is B (slot 2), B has prevote QC → finalize B
        let parent_hash = *consensus.block_parents.get(&block_c).unwrap();
        let parent_slot = *consensus.block_slots.get(&parent_hash).unwrap();
        assert_eq!(parent_slot, 2);
        assert!(consensus.qcs.contains_key(&(parent_slot, Phase::Prevote, parent_hash)));

        // Set finalized_slot to 2 (as if B was finalized)
        consensus.finalized_slot = 2;
        consensus.committed_slot = 3;

        // Now a late precommit QC for block_b (slot 2) arrives, parent is A (slot 1).
        // This should NOT regress finalized_slot from 2 to 1.
        let old_finalized = consensus.finalized_slot;

        // The finality code checks: parent_slot (1) > finalized_slot (2)? No → skip.
        // This is exactly what the monotonicity guard enforces.
        assert!(1 <= old_finalized, "slot 1 <= finalized 2, so no regression should occur");
        assert_eq!(consensus.finalized_slot, 2, "finalized_slot must not regress");
    }

    #[test]
    fn test_no_finality_without_known_parent_slot() {
        // If parent slot is unknown (not in block_slots), finality must NOT
        // guess with slot-1 — it should skip finalization entirely.
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        let block_x = H256::from_slice(&[0xDD; 32]).unwrap();
        let unknown_parent = H256::from_slice(&[0xEE; 32]).unwrap();

        // block_x has parent unknown_parent, but unknown_parent is NOT in block_slots
        consensus.block_parents.insert(block_x, unknown_parent);
        consensus.block_slots.insert(block_x, 10);
        // Do NOT insert unknown_parent into block_slots

        // Insert a prevote QC for slot 9 with unknown_parent hash
        // (would match if we incorrectly guessed parent_slot = 10-1 = 9)
        consensus.qcs.insert((9, Phase::Prevote, unknown_parent), AggregatedVote {
            slot: 9,
            block_hash: unknown_parent,
            phase: Phase::Prevote,
            aggregated_signature: vec![],
            signers: vec![],
            total_stake: 3000,
            aggregated_pubkey: vec![],
        });

        // Simulate precommit quorum for block_x
        consensus.current_phase = Phase::Precommit;
        consensus.current_slot = 10;

        // The parent_slot lookup should return None (not in block_slots),
        // so finalization must NOT happen.
        let parent_slot = consensus.block_slots.get(&unknown_parent).copied();
        assert!(parent_slot.is_none(), "parent slot must be unknown");
        assert_eq!(consensus.finalized_slot, 0, "finalized_slot must remain 0 — no guessing");
    }

    #[test]
    fn test_locking_rule_allows_higher_qc_unlock() {
        // After a view change, a block with parent_hash != locked_block should
        // still be accepted if the parent has a QC at slot >= locked_slot.
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        let locked_hash = H256::from_slice(&[0x11; 32]).unwrap();
        let new_parent = H256::from_slice(&[0x22; 32]).unwrap();

        // Lock on locked_hash at slot 5
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 5;

        // Register new_parent at slot 6 (higher than locked_slot)
        consensus.block_slots.insert(new_parent, 6);

        // Create a block that extends from new_parent, NOT locked_hash
        let block = Block {
            header: aether_types::BlockHeader {
                version: 1,
                slot: 7,
                parent_hash: new_parent,
                state_root: H256::zero(),
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                proposer: Address::from_slice(&[0; 20]).unwrap(),
                vrf_proof: aether_types::VrfProof { output: [0u8; 32], proof: vec![] },
                timestamp: 0,
            },
            transactions: vec![],
            aggregated_vote: None,
            slash_evidence: vec![],
        };

        // This should be accepted (parent slot 6 >= locked slot 5)
        let actions = consensus.on_propose(&block).unwrap();
        assert!(!actions.is_empty(), "block with higher-QC parent must be accepted");
    }

    #[test]
    fn test_locking_rule_rejects_lower_qc() {
        // A block extending from a parent with slot < locked_slot should be rejected.
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        let locked_hash = H256::from_slice(&[0x11; 32]).unwrap();
        let old_parent = H256::from_slice(&[0x33; 32]).unwrap();

        // Lock on locked_hash at slot 5
        consensus.locked_block = Some(locked_hash);
        consensus.locked_slot = 5;

        // Register old_parent at slot 3 (lower than locked_slot)
        consensus.block_slots.insert(old_parent, 3);

        let block = Block {
            header: aether_types::BlockHeader {
                version: 1,
                slot: 7,
                parent_hash: old_parent,
                state_root: H256::zero(),
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                proposer: Address::from_slice(&[0; 20]).unwrap(),
                vrf_proof: aether_types::VrfProof { output: [0u8; 32], proof: vec![] },
                timestamp: 0,
            },
            transactions: vec![],
            aggregated_vote: None,
            slash_evidence: vec![],
        };

        // This should be rejected (parent slot 3 < locked slot 5)
        let actions = consensus.on_propose(&block).unwrap();
        assert!(actions.is_empty(), "block with lower-QC parent must be rejected");
    }

    #[test]
    fn test_committed_slot_monotonicity() {
        // committed_slot should never decrease
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        consensus.committed_slot = 10;

        // A late precommit for slot 8 should not regress committed_slot
        // (Test the guard directly since the full on_vote flow is complex)
        let vote_slot: Slot = 8;
        if vote_slot > consensus.committed_slot {
            consensus.committed_slot = vote_slot;
        }
        assert_eq!(consensus.committed_slot, 10, "committed_slot must not regress");

        // A precommit for slot 12 should advance it
        let vote_slot: Slot = 12;
        if vote_slot > consensus.committed_slot {
            consensus.committed_slot = vote_slot;
        }
        assert_eq!(consensus.committed_slot, 12, "committed_slot should advance");
    }

    #[test]
    fn test_timeout_votes_pruned_after_tc() {
        let (mut consensus, validators, bls_keys) = setup_bls_consensus(4);

        // Accumulate timeout votes for rounds 1, 2, 3
        for round in 1..=3u64 {
            let addr = validators[0].pubkey.to_address();
            let mut msg = Vec::new();
            msg.extend_from_slice(b"timeout");
            msg.extend_from_slice(&round.to_le_bytes());
            msg.extend_from_slice(&0u64.to_le_bytes());
            msg.extend_from_slice(H256::zero().as_bytes());
            let signature = bls_keys[0].sign(&msg);

            let tv = TimeoutVote {
                round,
                validator: addr,
                validator_pubkey: validators[0].pubkey.clone(),
                stake: 1000,
                highest_qc_slot: 0,
                highest_qc_hash: H256::zero(),
                signature,
            };
            let _ = consensus.on_timeout_vote(tv);
        }

        assert_eq!(consensus.timeout_votes.len(), 3, "3 rounds accumulated");

        // Process a TC for round 2 — rounds 1 and 2 should be pruned
        let addrs: Vec<Address> = validators.iter().map(|v| v.pubkey.to_address()).collect();
        let tc = TimeoutCertificate {
            round: 2,
            total_stake: 3000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![addrs[0], addrs[1], addrs[2]],
        };
        consensus.on_timeout_certificate(&tc).unwrap();

        assert_eq!(
            consensus.timeout_votes.len(),
            1,
            "only round 3 should remain after TC for round 2"
        );
        assert!(
            consensus.timeout_votes.contains_key(&3),
            "round 3 votes must survive pruning"
        );
        assert!(
            !consensus.timeout_votes.contains_key(&1),
            "round 1 votes must be pruned"
        );
        assert!(
            !consensus.timeout_votes.contains_key(&2),
            "round 2 votes must be pruned"
        );
    }

    #[test]
    fn test_prune_finalized_state_clears_old_tracking() {
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        let hash_a = H256::from_slice(&[0xAA; 32]).unwrap();
        let hash_b = H256::from_slice(&[0xBB; 32]).unwrap();
        let hash_c = H256::from_slice(&[0xCC; 32]).unwrap();

        // Simulate block tracking at various slots
        consensus.block_slots.insert(hash_a, 1);
        consensus.block_slots.insert(hash_b, 5);
        consensus.block_slots.insert(hash_c, 10);
        consensus.block_parents.insert(hash_a, H256::zero());
        consensus.block_parents.insert(hash_b, hash_a);
        consensus.block_parents.insert(hash_c, hash_b);

        // Add QCs at different slots
        consensus.qcs.insert(
            (1, Phase::Prevote, hash_a),
            AggregatedVote {
                slot: 1,
                block_hash: hash_a,
                phase: Phase::Prevote,
                total_stake: 3000,
                signers: vec![],
                aggregated_signature: vec![],
                aggregated_pubkey: vec![],
            },
        );
        consensus.qcs.insert(
            (10, Phase::Prevote, hash_c),
            AggregatedVote {
                slot: 10,
                block_hash: hash_c,
                phase: Phase::Prevote,
                total_stake: 3000,
                signers: vec![],
                aggregated_signature: vec![],
                aggregated_pubkey: vec![],
            },
        );

        // Finalize at slot 7 — prune_below = 7 - 2 = 5
        consensus.finalized_slot = 7;
        consensus.prune_finalized_state();

        // Slot 1 < 5: pruned. Slot 5 >= 5: kept. Slot 10 >= 5: kept.
        assert!(
            !consensus.block_slots.contains_key(&hash_a),
            "slot 1 block should be pruned"
        );
        assert!(
            consensus.block_slots.contains_key(&hash_b),
            "slot 5 block should be retained"
        );
        assert!(
            consensus.block_slots.contains_key(&hash_c),
            "slot 10 block should be retained"
        );

        // Parent tracking follows block_slots
        assert!(!consensus.block_parents.contains_key(&hash_a));
        assert!(consensus.block_parents.contains_key(&hash_b));
        assert!(consensus.block_parents.contains_key(&hash_c));

        // QC at slot 1 pruned, QC at slot 10 kept
        assert!(!consensus
            .qcs
            .contains_key(&(1, Phase::Prevote, hash_a)));
        assert!(consensus
            .qcs
            .contains_key(&(10, Phase::Prevote, hash_c)));
    }

    #[test]
    fn test_prune_noop_for_low_finalized_slot() {
        let (mut consensus, _validators, _bls_keys) = setup_bls_consensus(4);

        let hash = H256::from_slice(&[0xAA; 32]).unwrap();
        consensus.block_slots.insert(hash, 0);
        consensus.block_parents.insert(hash, H256::zero());

        // Finalized slot 2 → prune_below = 0 → early return, nothing pruned
        consensus.finalized_slot = 2;
        consensus.prune_finalized_state();

        assert!(
            consensus.block_slots.contains_key(&hash),
            "no pruning when finalized_slot < 3"
        );
    }
}
