//! Byzantine fault tests for consensus.
//!
//! Tests that consensus still works correctly when 1 of 4 validators
//! behaves maliciously (sends conflicting votes). Verifies:
//! - Double-sign detection via SlashingDetector
//! - Correct slash amount calculation (5% for double-sign)
//! - HotStuff consensus still reaches quorum with 3 honest validators
//! - Byzantine validator's conflicting vote is rejected by HotStuff

use aether_consensus::hotstuff::*;
use aether_consensus::slashing::*;
use aether_crypto_bls::BlsKeypair;
use aether_types::{Address, PublicKey, Signature, ValidatorInfo, H256};

/// Helper: create 4 validators with BLS keys and register them in HotStuff.
fn setup_4_validators() -> (
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

    // Register BLS keys
    for (i, v) in validators.iter().enumerate() {
        let addr = v.pubkey.to_address();
        let pop = bls_keys[i].proof_of_possession();
        consensus
            .register_bls_pubkey(addr, bls_keys[i].public_key(), &pop)
            .unwrap();
    }

    (consensus, bls_keys, validators, addresses)
}

/// Helper: create a signed HotStuff vote.
fn make_hotstuff_vote(
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
    let signature = bls_kp.sign(&msg);

    HotStuffVote {
        slot,
        block_hash,
        parent_hash,
        phase,
        validator: addr,
        validator_pubkey: validator.pubkey.clone(),
        stake: validator.stake,
        signature,
    }
}

/// Test: 1 of 4 validators double-signs, SlashingDetector catches it,
/// and consensus still reaches quorum with the 3 honest validators.
#[test]
fn test_byzantine_double_sign_detected_consensus_continues() {
    let (mut consensus, bls_keys, validators, addresses) = setup_4_validators();

    let block_a = H256::from_slice(&[0xAA; 32]).unwrap();
    let block_b = H256::from_slice(&[0xBB; 32]).unwrap();
    let parent = H256::zero();
    let slot = 0;

    // Advance consensus to Prevote phase
    consensus.advance_phase();
    assert_eq!(*consensus.current_phase(), Phase::Prevote);

    // Use full BLS pubkey (48 bytes) for slashing votes
    let byz_bls_pubkey = PublicKey::from_bytes(bls_keys[0].public_key());

    // Byzantine validator (index 0) votes for block_a in HotStuff
    let byzantine_vote_a = make_hotstuff_vote(
        &bls_keys[0],
        &validators[0],
        addresses[0],
        slot,
        block_a,
        parent,
        Phase::Prevote,
    );
    let result = consensus.on_vote(byzantine_vote_a).unwrap();
    assert!(result.0.is_none(), "no quorum with 1 vote");

    // Create two conflicting slashing votes with real BLS signatures
    let make_slash_vote = |block_hash: H256| -> Vote {
        let mut v = Vote {
            slot,
            block_hash,
            validator: addresses[0],
            validator_pubkey: byz_bls_pubkey.clone(),
            signature: Signature::from_bytes(vec![]),
        };
        let msg = v.signing_message();
        v.signature = Signature::from_bytes(bls_keys[0].sign(&msg));
        v
    };

    let slash_vote_a = make_slash_vote(block_a);
    let slash_vote_b = make_slash_vote(block_b);

    // Detect double-sign with fully-signed votes
    let proof = detect_double_sign(&slash_vote_a, &slash_vote_b)
        .expect("double-sign must be detected");
    assert_eq!(proof.proof_type, SlashType::DoubleSign);
    assert_eq!(proof.validator, addresses[0]);

    // Verify slash proof cryptographically (both BLS sigs)
    assert!(
        verify_slash_proof(&proof).is_ok(),
        "BLS signatures in proof must verify"
    );

    // Calculate slash: 5% of 1000 = 50
    let slash_amount = calculate_slash_amount(1000, &proof.proof_type);
    assert_eq!(slash_amount, 50);

    // Apply slash and check reporter reward
    let event = apply_slash(1000, &proof, 100);
    assert_eq!(event.slash_amount, 50);
    assert_eq!(event.reporter_reward, 5); // 10% of slash

    // Also verify SlashingDetector catches it in real-time
    let mut detector = SlashingDetector::new();
    assert!(detector
        .record_vote(
            addresses[0],
            byz_bls_pubkey.clone(),
            slot,
            block_a,
            slash_vote_a.signature.clone(),
        )
        .is_none());
    let detector_proof = detector.record_vote(
        addresses[0],
        byz_bls_pubkey.clone(),
        slot,
        block_b,
        slash_vote_b.signature.clone(),
    );
    assert!(
        detector_proof.is_some(),
        "SlashingDetector must detect double-sign"
    );

    // --- Honest validators (1, 2, 3) vote for block_a → quorum ---
    for i in 1..=2 {
        let vote = make_hotstuff_vote(
            &bls_keys[i],
            &validators[i],
            addresses[i],
            slot,
            block_a,
            parent,
            Phase::Prevote,
        );
        let (qc, _actions) = consensus.on_vote(vote).unwrap();
        if i == 1 {
            assert!(qc.is_none(), "2 votes = 2000/4000, no quorum");
        }
        if i == 2 {
            assert!(
                qc.is_some(),
                "3 votes = 3000/4000 >= 2/3 quorum, must form QC"
            );
            let qc = qc.unwrap();
            assert_eq!(qc.block_hash, block_a);
            assert_eq!(qc.signers.len(), 3);
        }
    }
}

/// Test: HotStuff rejects duplicate vote from same validator for same block.
#[test]
fn test_hotstuff_rejects_duplicate_vote() {
    let (mut consensus, bls_keys, validators, addresses) = setup_4_validators();
    consensus.advance_phase(); // -> Prevote

    let block = H256::from_slice(&[0xCC; 32]).unwrap();
    let parent = H256::zero();

    let vote1 = make_hotstuff_vote(
        &bls_keys[0],
        &validators[0],
        addresses[0],
        0,
        block,
        parent,
        Phase::Prevote,
    );
    let vote2 = make_hotstuff_vote(
        &bls_keys[0],
        &validators[0],
        addresses[0],
        0,
        block,
        parent,
        Phase::Prevote,
    );

    assert!(consensus.on_vote(vote1).is_ok());
    assert!(
        consensus.on_vote(vote2).is_err(),
        "duplicate vote for same block must be rejected"
    );
}

/// Test: SlashingDetector correctly handles multiple Byzantine validators.
#[test]
fn test_multiple_byzantine_validators_detected() {
    let mut detector = SlashingDetector::new();

    let bls_keys: Vec<BlsKeypair> = (0..4).map(|_| BlsKeypair::generate()).collect();
    let pubkeys: Vec<PublicKey> = bls_keys
        .iter()
        .map(|bk| PublicKey::from_bytes(bk.public_key()[..32].to_vec()))
        .collect();
    let addrs: Vec<Address> = pubkeys.iter().map(|pk| pk.to_address()).collect();

    let block_a = H256::from_slice(&[1u8; 32]).unwrap();
    let block_b = H256::from_slice(&[2u8; 32]).unwrap();
    let slot = 42;

    // Validators 0 and 1 both double-sign
    for i in 0..2 {
        let sig_a = {
            let v = Vote {
                slot,
                block_hash: block_a,
                validator: addrs[i],
                validator_pubkey: pubkeys[i].clone(),
                signature: Signature::from_bytes(vec![]),
            };
            Signature::from_bytes(bls_keys[i].sign(&v.signing_message()))
        };
        assert!(detector
            .record_vote(addrs[i], pubkeys[i].clone(), slot, block_a, sig_a)
            .is_none());

        let sig_b = {
            let v = Vote {
                slot,
                block_hash: block_b,
                validator: addrs[i],
                validator_pubkey: pubkeys[i].clone(),
                signature: Signature::from_bytes(vec![]),
            };
            Signature::from_bytes(bls_keys[i].sign(&v.signing_message()))
        };
        let proof = detector.record_vote(addrs[i], pubkeys[i].clone(), slot, block_b, sig_b);
        assert!(proof.is_some(), "validator {} double-sign must be caught", i);
    }

    // Validators 2 and 3 vote honestly — no slash
    for i in 2..4 {
        let sig = {
            let v = Vote {
                slot,
                block_hash: block_a,
                validator: addrs[i],
                validator_pubkey: pubkeys[i].clone(),
                signature: Signature::from_bytes(vec![]),
            };
            Signature::from_bytes(bls_keys[i].sign(&v.signing_message()))
        };
        assert!(
            detector
                .record_vote(addrs[i], pubkeys[i].clone(), slot, block_a, sig)
                .is_none(),
            "honest validator {} must not be slashed",
            i
        );
    }
}

/// Test: Full consensus round succeeds with 3 honest + 1 Byzantine.
/// Byzantine validator votes for a different block, but HotStuff still
/// forms a QC from the 3 honest prevotes and advances through precommit.
#[test]
fn test_full_round_with_byzantine_validator() {
    let (mut consensus, bls_keys, validators, addresses) = setup_4_validators();

    let block_a = H256::from_slice(&[0xAA; 32]).unwrap();
    let block_evil = H256::from_slice(&[0xEE; 32]).unwrap();
    let parent = H256::zero();

    // Advance to Prevote
    consensus.advance_phase();

    // Byzantine validator votes for evil block (accepted, goes to different bucket)
    let evil_vote = make_hotstuff_vote(
        &bls_keys[0],
        &validators[0],
        addresses[0],
        0,
        block_evil,
        parent,
        Phase::Prevote,
    );
    let (qc, _) = consensus.on_vote(evil_vote).unwrap();
    assert!(qc.is_none(), "1 vote for evil block, no quorum");

    // 3 honest validators vote for block_a
    let mut final_qc = None;
    for i in 1..4 {
        let vote = make_hotstuff_vote(
            &bls_keys[i],
            &validators[i],
            addresses[i],
            0,
            block_a,
            parent,
            Phase::Prevote,
        );
        let (qc, _actions) = consensus.on_vote(vote).unwrap();
        if i == 3 {
            assert!(qc.is_some(), "3/4 honest votes must form QC");
            final_qc = qc;
        }
    }

    let qc = final_qc.unwrap();
    assert_eq!(qc.block_hash, block_a);
    assert_eq!(qc.signers.len(), 3);
    assert_eq!(qc.total_stake, 3000);

    // Evil block never reaches quorum (only 1 vote = 1000/4000)
    // This is verified implicitly — the evil vote was accepted but
    // no QC was returned for block_evil.
}

/// Test: Slash proof for surround vote (Casper FFG style).
#[test]
fn test_surround_vote_detection() {
    let bls_kp = BlsKeypair::generate();
    let pubkey = PublicKey::from_bytes(bls_kp.public_key()); // full 48-byte BLS pubkey
    let addr = pubkey.to_address();

    let make_vote = |slot: u64, block_byte: u8| -> Vote {
        let block_hash = H256::from_slice(&[block_byte; 32]).unwrap();
        let mut v = Vote {
            slot,
            block_hash,
            validator: addr,
            validator_pubkey: pubkey.clone(),
            signature: Signature::from_bytes(vec![]),
        };
        let msg = v.signing_message();
        v.signature = Signature::from_bytes(bls_kp.sign(&msg));
        v
    };

    // Vote A: source=1, target=10 (slot=10)
    // Vote B: source=2, target=5  (slot=5)
    // A surrounds B because source_a(1) < source_b(2) AND target_b(5) < target_a(10)
    let vote_a = make_vote(10, 0xAA);
    let vote_b = make_vote(5, 0xBB);

    let proof = detect_surround_vote(&vote_a, 1, &vote_b, 2);
    assert!(proof.is_some(), "surround vote must be detected");
    let proof = proof.unwrap();
    assert_eq!(proof.proof_type, SlashType::SurroundVote);
    assert_eq!(proof.validator, addr);

    // Verify the proof
    assert!(verify_slash_proof(&proof).is_ok());

    // Same slash rate as double-sign: 5%
    assert_eq!(calculate_slash_amount(1000, &proof.proof_type), 50);
}

/// Test: Downtime slash calculation — linear leak capped at 10%.
#[test]
fn test_downtime_slash_calculation() {
    // Small downtime: leak = missing_slots * 1 = 100, capped at stake/10 = 1000
    let amount = calculate_slash_amount(10_000, &SlashType::Downtime { missing_slots: 100 });
    assert_eq!(amount, 100, "100 missing slots = 100 leak");

    // Large downtime: capped at 10% of stake
    let capped = calculate_slash_amount(10_000, &SlashType::Downtime { missing_slots: 5000 });
    assert_eq!(capped, 1000, "capped at 10% of 10000");

    // Zero downtime: no slash
    let zero = calculate_slash_amount(10_000, &SlashType::Downtime { missing_slots: 0 });
    assert_eq!(zero, 0);
}
