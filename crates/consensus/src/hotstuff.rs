use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, BlsKeypair};
use aether_types::{Address, Block, PublicKey, Slot, ValidatorInfo, H256};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// HotStuff 2-Chain BFT Consensus
///
/// Provides Byzantine Fault Tolerant finality with optimal message complexity.
///
/// PHASES PER SLOT:
/// 1. PROPOSE: Leader broadcasts block
/// 2. PREVOTE: Validators vote if block extends from locked block
/// 3. PRECOMMIT: Validators vote if prevote has 2/3 quorum
/// 4. COMMIT: Finalize if precommit has 2/3 quorum
///
/// SAFETY: Cannot finalize two conflicting blocks
/// LIVENESS: Progress under synchrony (Δ network delay)
///
/// 2-CHAIN RULE:
/// - Block B is committed if:
///   1. B has 2/3 prevote quorum
///   2. B.child has 2/3 precommit quorum
///   3. Both within 2 consecutive slots
///
/// INTEGRATION WITH VRF-PoS:
/// - VRF determines slot leaders
/// - BLS aggregates validator votes
/// - Quorum weighted by stake (not count)

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Phase {
    Propose,
    Prevote,
    Precommit,
    Commit,
}

#[derive(Debug, Clone)]
pub struct HotStuffVote {
    pub slot: Slot,
    pub block_hash: H256,
    pub phase: Phase,
    pub validator: Address,
    pub validator_pubkey: PublicKey,
    pub stake: u128,
    pub signature: Vec<u8>, // BLS signature
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
    /// Current phase of consensus
    current_phase: Phase,

    /// Current slot
    current_slot: Slot,

    /// Validator set with stakes
    validators: HashMap<Address, ValidatorInfo>,

    /// Total stake
    total_stake: u128,

    /// Votes received for current slot (phase -> block_hash -> votes)
    votes: HashMap<Phase, HashMap<H256, Vec<HotStuffVote>>>,

    /// Aggregated votes (quorum certificates)
    qcs: HashMap<(Slot, Phase, H256), AggregatedVote>,

    /// Locked block (cannot vote for conflicting blocks)
    locked_block: Option<H256>,
    locked_slot: Slot,

    /// Highest committed slot
    committed_slot: Slot,

    /// Highest finalized slot (irreversible)
    finalized_slot: Slot,

    /// My validator keypair (if I'm a validator)
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
            locked_block: None,
            locked_slot: 0,
            committed_slot: 0,
            finalized_slot: 0,
            my_keypair,
            my_address,
        }
    }

    /// Advance to next phase
    pub fn advance_phase(&mut self) {
        self.current_phase = match self.current_phase {
            Phase::Propose => Phase::Prevote,
            Phase::Prevote => Phase::Precommit,
            Phase::Precommit => Phase::Commit,
            Phase::Commit => {
                // Move to next slot
                self.current_slot += 1;
                self.votes.clear();
                Phase::Propose
            }
        };
    }

    /// Process a proposed block
    pub fn on_propose(&mut self, block: &Block) -> Result<Option<HotStuffVote>> {
        if self.current_phase != Phase::Propose {
            bail!("not in propose phase");
        }

        // Validate block extends from our locked block (if any)
        if let Some(locked) = &self.locked_block {
            if block.header.parent_hash != *locked {
                // Cannot vote for block that doesn't extend locked block
                return Ok(None);
            }
        }

        // Create prevote
        self.advance_phase();
        self.create_vote(block.hash(), Phase::Prevote)
    }

    /// Process votes and check for quorum
    pub fn on_vote(&mut self, vote: HotStuffVote) -> Result<Option<AggregatedVote>> {
        // Verify vote signature
        self.verify_vote(&vote)?;

        // Store vote
        let phase_votes = self
            .votes
            .entry(vote.phase.clone())
            .or_insert_with(HashMap::new);
        let block_votes = phase_votes.entry(vote.block_hash).or_insert_with(Vec::new);
        block_votes.push(vote.clone());

        // Check for quorum (calculate stake and check threshold before any other borrows)
        let stake: u128 = block_votes.iter().map(|v| v.stake).sum();
        let total_stake = self.total_stake;
        let has_quorum = stake * 3 >= total_stake * 2;

        if has_quorum {
            // Clone votes for aggregation to avoid borrow conflicts
            let votes_to_aggregate = block_votes.clone();
            // Create quorum certificate (QC)
            let qc = self.aggregate_votes(&votes_to_aggregate)?;

            // Store QC
            self.qcs
                .insert((vote.slot, vote.phase.clone(), vote.block_hash), qc.clone());

            // Handle phase-specific logic
            match vote.phase {
                Phase::Prevote => {
                    // Lock on this block
                    self.locked_block = Some(vote.block_hash);
                    self.locked_slot = vote.slot;

                    // Advance to precommit
                    self.advance_phase();

                    // Create precommit vote
                    if let Some(my_vote) = self.create_vote(vote.block_hash, Phase::Precommit)? {
                        // In production, broadcast this vote
                        self.on_vote(my_vote)?;
                    }
                }
                Phase::Precommit => {
                    // Check 2-chain rule for finality
                    if let Some(parent_slot) = vote.slot.checked_sub(1) {
                        // Check if parent block has prevote QC
                        if self
                            .qcs
                            .contains_key(&(parent_slot, Phase::Prevote, vote.block_hash))
                        {
                            // Finalize!
                            self.finalized_slot = parent_slot;
                            println!("FINALIZED slot {} block {:?}", parent_slot, vote.block_hash);
                        }
                    }

                    // Mark as committed
                    self.committed_slot = vote.slot;

                    // Advance to commit
                    self.advance_phase();
                }
                _ => {}
            }

            return Ok(Some(qc));
        }

        Ok(None)
    }

    /// Create a vote for a block
    fn create_vote(&self, block_hash: H256, phase: Phase) -> Result<Option<HotStuffVote>> {
        let (keypair, address) = match (&self.my_keypair, &self.my_address) {
            (Some(kp), Some(addr)) => (kp, addr),
            _ => return Ok(None), // Not a validator
        };

        let validator = self
            .validators
            .get(address)
            .ok_or_else(|| anyhow::anyhow!("not in validator set"))?;

        // Create vote message
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&self.current_slot.to_le_bytes());
        msg.extend_from_slice(&format!("{:?}", phase).as_bytes());

        // Sign with BLS
        let signature = keypair.sign(&msg);

        Ok(Some(HotStuffVote {
            slot: self.current_slot,
            block_hash,
            phase,
            validator: address.clone(),
            validator_pubkey: validator.pubkey.clone(),
            stake: validator.stake,
            signature,
        }))
    }

    /// Verify a vote's signature
    fn verify_vote(&self, vote: &HotStuffVote) -> Result<()> {
        let validator = self
            .validators
            .get(&vote.validator)
            .ok_or_else(|| anyhow::anyhow!("unknown validator"))?;

        // Reconstruct message
        let mut msg = Vec::new();
        msg.extend_from_slice(vote.block_hash.as_bytes());
        msg.extend_from_slice(&vote.slot.to_le_bytes());
        msg.extend_from_slice(&format!("{:?}", vote.phase).as_bytes());

        // Verify BLS signature
        let pubkey_bytes: [u8; 48] = validator.pubkey.as_bytes()[..48]
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid pubkey length"))?;
        aether_crypto_bls::keypair::verify(&pubkey_bytes, &msg, &vote.signature)?;

        Ok(())
    }

    /// Aggregate votes into a quorum certificate
    fn aggregate_votes(&self, votes: &[HotStuffVote]) -> Result<AggregatedVote> {
        let signatures: Vec<Vec<u8>> = votes.iter().map(|v| v.signature.clone()).collect();
        let pubkeys: Vec<Vec<u8>> = votes
            .iter()
            .map(|v| {
                let bytes = v.validator_pubkey.as_bytes();
                // BLS requires 48 bytes, pad if necessary
                let mut padded = vec![0u8; 48];
                let copy_len = bytes.len().min(48);
                padded[..copy_len].copy_from_slice(&bytes[..copy_len]);
                padded
            })
            .collect();

        let agg_sig = aggregate_signatures(&signatures)?;
        let agg_pk = aggregate_public_keys(&pubkeys)?;

        let total_stake = votes.iter().map(|v| v.stake).sum();
        let signers = votes.iter().map(|v| v.validator).collect();

        Ok(AggregatedVote {
            slot: votes[0].slot,
            block_hash: votes[0].block_hash,
            phase: votes[0].phase.clone(),
            total_stake,
            signers,
            aggregated_signature: agg_sig,
            aggregated_pubkey: agg_pk,
        })
    }

    /// Check if stake reaches 2/3 quorum
    #[cfg(test)]
    fn has_quorum(&self, stake: u128) -> bool {
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

        // 2/3 of 4000 = 2667
        assert!(!consensus.has_quorum(2666)); // Just below
        assert!(consensus.has_quorum(2667)); // Exactly 2/3
        assert!(consensus.has_quorum(3000)); // Above
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
        let consensus = HotStuffConsensus::new(validators.clone(), None, None);

        // 2/3 of 3000 = 2000, so we need >= 2000 for quorum
        assert!(!consensus.has_quorum(1999));
        assert!(consensus.has_quorum(2000));
        assert!(consensus.has_quorum(3000));
    }
}
