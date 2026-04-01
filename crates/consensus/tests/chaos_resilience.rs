//! Chaos tests for consensus resilience.
//!
//! These tests simulate adverse conditions (leader crash, Byzantine votes,
//! network delays) using in-process consensus instances — no Docker required.

use aether_consensus::hotstuff::*;
use aether_consensus::pacemaker::Pacemaker;
use aether_types::{PublicKey, ValidatorInfo, H256};
use std::time::Duration;

fn create_validators(count: usize) -> Vec<ValidatorInfo> {
    (0..count)
        .map(|_| {
            let kp = aether_crypto_primitives::Keypair::generate();
            ValidatorInfo {
                pubkey: PublicKey::from_bytes(kp.public_key().to_vec()),
                stake: 1000,
                commission: 0,
                active: true,
            }
        })
        .collect()
}

/// Test: Pacemaker timeout fires when leader doesn't propose.
///
/// Simulates leader crash by never calling on_propose(). The pacemaker
/// should detect the timeout and trigger a view change.
#[test]
fn test_leader_crash_triggers_timeout() {
    let validators = create_validators(4);
    let mut pacemaker = Pacemaker::new(Duration::from_millis(10));

    // Simulate waiting longer than timeout
    std::thread::sleep(Duration::from_millis(20));

    assert!(
        pacemaker.is_timed_out(),
        "pacemaker should timeout when leader doesn't propose"
    );

    // Trigger timeout → advances to next round
    pacemaker.on_timeout();
    assert_eq!(pacemaker.current_round(), 1);

    // New leader is different from the crashed one
    let old_leader = pacemaker.leader_for_round(0, validators.len());
    let new_leader = pacemaker.leader_for_round(1, validators.len());
    assert_ne!(
        old_leader, new_leader,
        "new round should have a different leader"
    );
}

/// Test: Timeout certificate forms with 2/3 stake.
///
/// Simulates 3 of 4 validators sending timeout votes with proper BLS signatures.
#[test]
fn test_timeout_certificate_quorum() {
    use aether_crypto_bls::BlsKeypair;

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

    let mut consensus = HotStuffConsensus::new(validators.clone(), None, None);

    // Register BLS keys for all validators
    for (i, v) in validators.iter().enumerate() {
        let addr = v.pubkey.to_address();
        let pop = bls_keys[i].proof_of_possession();
        consensus
            .register_bls_pubkey(addr, bls_keys[i].public_key(), &pop)
            .unwrap();
    }

    // Collect timeout votes from 3 validators
    for (i, validator) in validators.iter().take(3).enumerate() {
        let addr = validator.pubkey.to_address();
        // Sign the correct timeout message format
        let mut msg = Vec::new();
        msg.extend_from_slice(b"timeout");
        msg.extend_from_slice(&1u64.to_le_bytes());
        msg.extend_from_slice(&0u64.to_le_bytes());
        msg.extend_from_slice(H256::zero().as_bytes());
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
        if i == 2 {
            // 3000/4000 = 75% > 66.7% quorum
            assert!(result.is_some(), "TC should form with 3/4 stake");
            let tc = result.unwrap();
            assert_eq!(tc.round, 1);
            assert_eq!(tc.signers.len(), 3);
        }
    }
}

/// Test: Exponential backoff on consecutive timeouts.
///
/// Simulates repeated leader failures. Timeout should double each time
/// but cap at maximum.
#[test]
fn test_exponential_backoff() {
    let mut pacemaker = Pacemaker::new(Duration::from_millis(100));

    assert_eq!(pacemaker.current_timeout(), Duration::from_millis(100));

    pacemaker.on_timeout();
    assert_eq!(pacemaker.current_timeout(), Duration::from_millis(200));

    pacemaker.on_timeout();
    assert_eq!(pacemaker.current_timeout(), Duration::from_millis(400));

    pacemaker.on_timeout();
    assert_eq!(pacemaker.current_timeout(), Duration::from_millis(800));

    // After a successful commit, timeout resets
    pacemaker.on_commit();
    assert_eq!(pacemaker.current_timeout(), Duration::from_millis(100));
}

/// Test: Conflicting votes from same validator are detectable.
///
/// A Byzantine validator sends two different votes for the same slot.
/// The slashing module should detect this.
#[test]
fn test_byzantine_double_vote_detected() {
    use aether_consensus::slashing::*;

    // Use BLS keypair since votes are BLS-signed
    let bls_kp = aether_crypto_bls::BlsKeypair::generate();
    let bls_pubkey_bytes = bls_kp.public_key();
    let pubkey = PublicKey::from_bytes(bls_pubkey_bytes.clone());
    let validator = pubkey.to_address();

    // Create two conflicting votes for the same slot
    let make_vote = |block_byte: u8| -> Vote {
        let block_hash = H256::from_slice(&[block_byte; 32]).unwrap();
        let mut v = Vote {
            slot: 100,
            block_hash,
            validator,
            validator_pubkey: pubkey.clone(),
            signature: aether_types::Signature::from_bytes(vec![]),
        };
        let msg = v.signing_message();
        v.signature = aether_types::Signature::from_bytes(bls_kp.sign(&msg));
        v
    };

    let vote1 = make_vote(1);
    let vote2 = make_vote(2);

    // Detect the double sign
    let proof = detect_double_sign(&vote1, &vote2);
    assert!(proof.is_some(), "double sign must be detected");

    // Verify the proof (checks real BLS signatures)
    let proof = proof.unwrap();
    assert!(verify_slash_proof(&proof).is_ok(), "proof must verify");
}

/// Test: Timeout certificate advances the slot.
///
/// After a TC forms, the consensus should move to the next slot
/// and reset to Propose phase.
#[test]
fn test_tc_advances_consensus() {
    let validators = create_validators(4);
    let addrs: Vec<_> = validators.iter().map(|v| v.pubkey.to_address()).collect();
    let mut consensus = HotStuffConsensus::new(validators, None, None);

    assert_eq!(consensus.current_slot(), 0);
    assert_eq!(*consensus.current_phase(), Phase::Propose);

    let tc = TimeoutCertificate {
        round: 1,
        total_stake: 3000,
        highest_qc_slot: 0,
        highest_qc_hash: H256::zero(),
        signers: vec![addrs[0], addrs[1], addrs[2]],
    };

    consensus.on_timeout_certificate(&tc).unwrap();
    assert_eq!(consensus.current_slot(), 1, "slot should advance after TC");
    assert_eq!(
        *consensus.current_phase(),
        Phase::Propose,
        "should reset to Propose phase"
    );
}
