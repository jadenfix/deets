//! End-to-end tests for the HotStuff 2-chain finality rule.
//!
//! The 2-chain rule: a block is finalized when its child receives a
//! precommit QC AND the parent already has a prevote QC.
//!
//! These tests exercise the full on_vote() path rather than manually
//! setting finalized_slot, ensuring the finality logic is wired correctly.

use aether_consensus::hotstuff::*;
use aether_crypto_bls::BlsKeypair;
use aether_types::{Address, Block, PublicKey, ValidatorInfo, VrfProof, H256};

/// Setup 4 validators with BLS keys registered in HotStuff.
fn setup_validators() -> (
    HotStuffConsensus,
    Vec<BlsKeypair>,
    Vec<ValidatorInfo>,
    Vec<Address>,
) {
    let bls_keys: Vec<BlsKeypair> = (0..4).map(|_| BlsKeypair::generate()).collect();
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
    let addresses: Vec<Address> = validators.iter().map(|v| v.pubkey.to_address()).collect();
    let mut consensus = HotStuffConsensus::new(validators.clone(), None, None);
    for (i, v) in validators.iter().enumerate() {
        let addr = v.pubkey.to_address();
        let pop = bls_keys[i].proof_of_possession();
        consensus
            .register_bls_pubkey(addr, bls_keys[i].public_key(), &pop)
            .unwrap();
    }
    (consensus, bls_keys, validators, addresses)
}

/// Create a signed HotStuff vote with correct canonical message format.
fn signed_vote(
    bls_kp: &BlsKeypair,
    validator: &ValidatorInfo,
    addr: Address,
    slot: u64,
    block_hash: H256,
    parent_hash: H256,
    phase: Phase,
) -> HotStuffVote {
    let phase_byte = match &phase {
        Phase::Propose => 0u8,
        Phase::Prevote => 1,
        Phase::Precommit => 2,
        Phase::Commit => 3,
    };
    let mut msg = Vec::new();
    msg.extend_from_slice(block_hash.as_bytes());
    msg.extend_from_slice(parent_hash.as_bytes());
    msg.extend_from_slice(&slot.to_le_bytes());
    msg.push(phase_byte);

    HotStuffVote {
        slot,
        block_hash,
        parent_hash,
        phase,
        validator: addr,
        validator_pubkey: validator.pubkey.clone(),
        stake: validator.stake,
        signature: bls_kp.sign(&msg),
    }
}

/// Create a minimal block for on_propose().
fn make_block(slot: u64, parent_hash: H256, proposer: Address) -> Block {
    Block::new(
        slot,
        parent_hash,
        proposer,
        VrfProof {
            output: [0u8; 32],
            proof: vec![],
        },
        vec![],
    )
}

/// Run a full round for a block: propose, prevote quorum, precommit quorum,
/// then advance through Commit phase back to Propose for next slot.
/// Returns all ConsensusActions emitted during the round.
fn run_full_round(
    consensus: &mut HotStuffConsensus,
    bls_keys: &[BlsKeypair],
    validators: &[ValidatorInfo],
    addresses: &[Address],
    block: &Block,
) -> Vec<ConsensusAction> {
    let block_hash = block.hash();
    let parent_hash = block.header.parent_hash;
    let slot = block.header.slot;
    let mut all_actions = Vec::new();

    // Propose (Propose → Prevote)
    let propose_actions = consensus.on_propose(block).unwrap();
    all_actions.extend(propose_actions);

    // Prevote quorum (Prevote → Precommit)
    for i in 0..3 {
        let vote = signed_vote(
            &bls_keys[i],
            &validators[i],
            addresses[i],
            slot,
            block_hash,
            parent_hash,
            Phase::Prevote,
        );
        let (qc, actions) = consensus.on_vote(vote).unwrap();
        all_actions.extend(actions);
        if i == 2 {
            assert!(qc.is_some(), "3/4 prevotes must form QC at slot {}", slot);
        }
    }

    // Precommit quorum (Precommit → Commit)
    for i in 0..3 {
        let vote = signed_vote(
            &bls_keys[i],
            &validators[i],
            addresses[i],
            slot,
            block_hash,
            parent_hash,
            Phase::Precommit,
        );
        let (qc, actions) = consensus.on_vote(vote).unwrap();
        all_actions.extend(actions);
        if i == 2 {
            assert!(qc.is_some(), "3/4 precommits must form QC at slot {}", slot);
        }
    }

    // Advance from Commit → Propose (next slot)
    consensus.advance_phase();

    all_actions
}

/// Core test: block A gets prevote QC, block B (child of A) gets precommit QC,
/// which triggers finalization of block A via the 2-chain rule.
#[test]
fn test_2chain_finality_finalizes_parent_on_child_precommit() {
    let (mut consensus, bls_keys, validators, addresses) = setup_validators();
    let genesis = H256::zero();

    // Slot 1: full round for block A — no finalization (genesis has no prevote QC)
    let block_a = make_block(1, genesis, addresses[0]);
    let hash_a = block_a.hash();
    let actions_a = run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_a);
    assert!(
        !actions_a
            .iter()
            .any(|a| matches!(a, ConsensusAction::Finalized { .. })),
        "block A cannot be finalized yet — genesis parent has no prevote QC"
    );

    // Slot 2: full round for block B (child of A) — should finalize A
    let block_b = make_block(2, hash_a, addresses[1]);
    let actions_b = run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_b);

    let finalized: Vec<_> = actions_b
        .iter()
        .filter_map(|a| match a {
            ConsensusAction::Finalized { slot, block_hash } => Some((*slot, *block_hash)),
            _ => None,
        })
        .collect();
    assert_eq!(finalized.len(), 1, "exactly one block should be finalized");
    assert_eq!(finalized[0].0, 1, "finalized slot must be block A's slot");
    assert_eq!(finalized[0].1, hash_a, "finalized hash must be block A");
    assert_eq!(consensus.finalized_slot(), 1);
}

/// Test: 3-block chain, finality advances A → B.
#[test]
fn test_2chain_finality_advances_through_chain() {
    let (mut consensus, bls_keys, validators, addresses) = setup_validators();
    let genesis = H256::zero();

    let block_a = make_block(1, genesis, addresses[0]);
    let hash_a = block_a.hash();
    run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_a);

    let block_b = make_block(2, hash_a, addresses[1]);
    let hash_b = block_b.hash();
    let actions_b = run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_b);
    assert!(actions_b
        .iter()
        .any(|a| matches!(a, ConsensusAction::Finalized { slot: 1, .. })));

    let block_c = make_block(3, hash_b, addresses[2]);
    let actions_c = run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_c);
    assert!(actions_c
        .iter()
        .any(|a| matches!(a, ConsensusAction::Finalized { slot: 2, .. })));
    assert_eq!(consensus.finalized_slot(), 2);
}

/// Test: finality never regresses (monotonicity).
#[test]
fn test_2chain_finality_monotonicity() {
    let (mut consensus, bls_keys, validators, addresses) = setup_validators();
    let genesis = H256::zero();

    // Chain: genesis → A(1) → B(2) → C(3) → D(4)
    let block_a = make_block(1, genesis, addresses[0]);
    let hash_a = block_a.hash();
    run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_a);

    let block_b = make_block(2, hash_a, addresses[1]);
    let hash_b = block_b.hash();
    run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_b);
    assert_eq!(consensus.finalized_slot(), 1);

    let block_c = make_block(3, hash_b, addresses[2]);
    let hash_c = block_c.hash();
    run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_c);
    assert_eq!(consensus.finalized_slot(), 2);

    let block_d = make_block(4, hash_c, addresses[3]);
    run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_d);
    assert_eq!(
        consensus.finalized_slot(),
        3,
        "finality must advance monotonically"
    );
}

/// Test: skipped slots don't break finality.
#[test]
fn test_2chain_finality_with_skipped_slot() {
    let (mut consensus, bls_keys, validators, addresses) = setup_validators();
    let genesis = H256::zero();

    // Block A at slot 1
    let block_a = make_block(1, genesis, addresses[0]);
    let hash_a = block_a.hash();
    run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_a);

    // Skip slot 2 — block B at slot 3 extends A
    let block_b = make_block(3, hash_a, addresses[1]);
    let actions = run_full_round(&mut consensus, &bls_keys, &validators, &addresses, &block_b);

    let finalized_slots: Vec<u64> = actions
        .iter()
        .filter_map(|a| match a {
            ConsensusAction::Finalized { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect();
    assert!(
        finalized_slots.contains(&1),
        "block A must be finalized despite skipped slot 2"
    );
}
