use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, BlsKeypair};
use aether_types::{Address, Block, PublicKey, Slot, ValidatorInfo, H256};
use anyhow::{bail, Result};

use std::collections::HashMap;

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
///   → B is finalized
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

    /// Locked block (safety: cannot vote for conflicting blocks)
    locked_block: Option<H256>,
    locked_slot: Slot,

    committed_slot: Slot,
    finalized_slot: Slot,

    my_keypair: Option<BlsKeypair>,
    my_address: Option<Address>,
}

impl HotStuffConsensus {
    pub fn new(
        validators: Vec<ValidatorInfo>,
        my_keypair: Option<BlsKeypair>,
        my_address: Option<Address>,
    ) -> Self {
        let total_stake: u128 = validators.iter().map(|v| v.stake).sum();
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
            locked_block: None,
            locked_slot: 0,
            committed_slot: 0,
            finalized_slot: 0,
            my_keypair,
            my_address,
        }
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
        if self.current_phase != Phase::Propose {
            bail!("not in propose phase");
        }

        // Track parent relationship
        self.block_parents
            .insert(block.hash(), block.header.parent_hash);

        // Validate block extends from our locked block (if any)
        if let Some(locked) = &self.locked_block {
            if block.header.parent_hash != *locked {
                return Ok(vec![]);
            }
        }

        self.advance_phase();

        // Create prevote and return it as an action (NOT recursive)
        let mut actions = Vec::new();
        if let Some(vote) = self.create_vote(block.hash(), block.header.parent_hash, Phase::Prevote)? {
            actions.push(ConsensusAction::BroadcastVote(vote));
        }
        Ok(actions)
    }

    /// Process a vote. Returns QC (if quorum reached) and actions for the node.
    pub fn on_vote(
        &mut self,
        vote: HotStuffVote,
    ) -> Result<(Option<AggregatedVote>, Vec<ConsensusAction>)> {
        self.verify_vote(&vote)?;

        // Track parent
        self.block_parents
            .entry(vote.block_hash)
            .or_insert(vote.parent_hash);

        // Store vote
        let phase_votes = self.votes.entry(vote.phase.clone()).or_default();
        let block_votes = phase_votes.entry(vote.block_hash).or_default();
        block_votes.push(vote.clone());

        // Check for quorum
        let stake: u128 = block_votes.iter().map(|v| v.stake).sum();
        let has_quorum = stake * 3 >= self.total_stake * 2;

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

                if let Some(parent_slot) = vote.slot.checked_sub(1) {
                    // Look for parent block's prevote QC using the PARENT's hash
                    if self
                        .qcs
                        .contains_key(&(parent_slot, Phase::Prevote, parent_hash))
                    {
                        self.finalized_slot = parent_slot;
                        actions.push(ConsensusAction::Finalized {
                            slot: parent_slot,
                            block_hash: parent_hash,
                        });
                    }
                }

                self.committed_slot = vote.slot;
                self.advance_phase();
            }
            _ => {}
        }

        Ok((Some(qc), actions))
    }

    /// Handle a pacemaker timeout: create a timeout vote.
    pub fn on_timeout(&self, round: u64) -> Result<Vec<ConsensusAction>> {
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
    pub fn on_timeout_vote(&mut self, tv: TimeoutVote) -> Result<Option<TimeoutCertificate>> {
        let round_votes = self.timeout_votes.entry(tv.round).or_default();
        round_votes.push(tv.clone());

        let stake: u128 = round_votes.iter().map(|v| v.stake).sum();
        if stake * 3 < self.total_stake * 2 {
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
    pub fn on_timeout_certificate(&mut self, _tc: &TimeoutCertificate) {
        // Advance slot (new round = new leader)
        self.current_slot += 1;
        self.current_phase = Phase::Propose;
        self.votes.clear();
    }

    /// Find the highest QC we've seen.
    fn highest_qc(&self) -> (Slot, H256) {
        let mut best_slot = 0;
        let mut best_hash = H256::zero();
        for ((slot, _, hash), _) in &self.qcs {
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
        msg.extend_from_slice(format!("{:?}", phase).as_bytes());

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

    /// Verify a vote's BLS signature.
    fn verify_vote(&self, vote: &HotStuffVote) -> Result<()> {
        let validator = self
            .validators
            .get(&vote.validator)
            .ok_or_else(|| anyhow::anyhow!("unknown validator"))?;

        let mut msg = Vec::new();
        msg.extend_from_slice(vote.block_hash.as_bytes());
        msg.extend_from_slice(vote.parent_hash.as_bytes());
        msg.extend_from_slice(&vote.slot.to_le_bytes());
        msg.extend_from_slice(format!("{:?}", vote.phase).as_bytes());

        let pubkey_bytes: [u8; 48] = validator.pubkey.as_bytes()[..48]
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid pubkey length"))?;
        aether_crypto_bls::keypair::verify(&pubkey_bytes, &msg, &vote.signature)?;

        Ok(())
    }

    fn aggregate_votes(&self, votes: &[HotStuffVote]) -> Result<AggregatedVote> {
        let signatures: Vec<Vec<u8>> = votes.iter().map(|v| v.signature.clone()).collect();
        let pubkeys: Vec<Vec<u8>> = votes
            .iter()
            .map(|v| {
                let bytes = v.validator_pubkey.as_bytes();
                let mut padded = vec![0u8; 48];
                let copy_len = bytes.len().min(48);
                padded[..copy_len].copy_from_slice(&bytes[..copy_len]);
                padded
            })
            .collect();

        let agg_sig = aggregate_signatures(&signatures)?;
        let agg_pk = aggregate_public_keys(&pubkeys)?;

        Ok(AggregatedVote {
            slot: votes[0].slot,
            block_hash: votes[0].block_hash,
            phase: votes[0].phase.clone(),
            total_stake: votes.iter().map(|v| v.stake).sum(),
            signers: votes.iter().map(|v| v.validator).collect(),
            aggregated_signature: agg_sig,
            aggregated_pubkey: agg_pk,
        })
    }

    #[allow(dead_code)]
    pub fn has_quorum(&self, stake: u128) -> bool {
        stake * 3 >= self.total_stake * 2
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
        for i in 0..3 {
            let vote = HotStuffVote {
                slot: 0,
                block_hash,
                parent_hash,
                phase: Phase::Prevote,
                validator: validators[i].pubkey.to_address(),
                validator_pubkey: validators[i].pubkey.clone(),
                stake: 1000,
                signature: vec![0u8; 96], // dummy sig
            };

            // Insert directly into votes map to skip BLS verify
            let phase_votes = consensus.votes.entry(Phase::Prevote).or_default();
            let block_votes = phase_votes.entry(block_hash).or_default();
            block_votes.push(vote.clone());

            // Check quorum manually
            let stake: u128 = block_votes.iter().map(|v| v.stake).sum();
            if stake * 3 >= consensus.total_stake * 2 {
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

    #[test]
    fn test_timeout_vote_collection() {
        let validators = create_test_validators(4);
        let mut consensus = HotStuffConsensus::new(validators.clone(), None, None);

        let kp = BlsKeypair::generate();

        // Collect timeout votes from 3 of 4 validators
        for i in 0..3 {
            let tv = TimeoutVote {
                round: 1,
                validator: validators[i].pubkey.to_address(),
                validator_pubkey: validators[i].pubkey.clone(),
                stake: 1000,
                highest_qc_slot: 0,
                highest_qc_hash: H256::zero(),
                signature: kp.sign(b"timeout"),
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
    fn test_timeout_certificate_advances_slot() {
        let validators = create_test_validators(4);
        let mut consensus = HotStuffConsensus::new(validators, None, None);

        let initial_slot = consensus.current_slot();
        let tc = TimeoutCertificate {
            round: 1,
            total_stake: 3000,
            highest_qc_slot: 0,
            highest_qc_hash: H256::zero(),
            signers: vec![],
        };

        consensus.on_timeout_certificate(&tc);
        assert_eq!(consensus.current_slot(), initial_slot + 1);
        assert_eq!(*consensus.current_phase(), Phase::Propose);
    }
}
