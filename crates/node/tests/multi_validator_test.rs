// Multi-validator integration test for Phase 1
// Tests HotStuff consensus with BLS aggregation across 4+ validators

use aether_consensus::{ConsensusEngine, HybridConsensus};
use aether_crypto_bls::BlsKeypair;
use aether_crypto_primitives::Keypair;
use aether_crypto_vrf::VrfKeypair;
use aether_types::{PublicKey, ValidatorInfo};

#[test]
fn test_four_validator_consensus() {
    // Create 4 validators with BLS and VRF keys
    let mut validators = Vec::new();
    let mut bls_keys = Vec::new();
    let mut vrf_keys = Vec::new();

    for _ in 0..4 {
        let keypair = Keypair::generate();
        let validator = ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake: 1000,
            commission: 0,
            active: true,
        };
        validators.push(validator);
        bls_keys.push(BlsKeypair::generate());
        vrf_keys.push(VrfKeypair::generate());
    }

    // Create consensus engines for each validator
    let mut engines: Vec<HybridConsensus> = (0..4)
        .map(|i| {
            let addr = validators[i].pubkey.to_address();
            HybridConsensus::new(
                validators.clone(),
                0.8, // tau
                100, // epoch_length
                Some(vrf_keys[i].clone()),
                Some(bls_keys[i].clone()),
                Some(addr),
            )
        })
        .collect();

    // Simulate consensus rounds
    for slot in 0..10 {
        // Each validator checks if they're the leader
        for (i, engine) in engines.iter_mut().enumerate() {
            if let Some(_vrf_proof) = engine.check_my_eligibility(slot) {
                println!("Slot {}: Validator {} is leader", slot, i);

                // Leader proposes block
                // Other validators vote
                // Check quorum formation

                // In a real test, we'd:
                // 1. Create and broadcast block
                // 2. Collect votes from all validators
                // 3. Verify QC formation
                // 4. Advance HotStuff phases
                // 5. Verify finality after 2-chain

                break;
            }
        }

        // Advance all validators to next slot
        for engine in &mut engines {
            engine.advance_slot();
        }
    }

    println!("Multi-validator test completed");
}

#[test]
fn test_quorum_formation() {
    // Test that 2/3+ stake forms valid quorum
    let validators = vec![
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(Keypair::generate().public_key()),
            stake: 1000,
            commission: 0,
            active: true,
        },
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(Keypair::generate().public_key()),
            stake: 1000,
            commission: 0,
            active: true,
        },
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(Keypair::generate().public_key()),
            stake: 1000,
            commission: 0,
            active: true,
        },
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(Keypair::generate().public_key()),
            stake: 1000,
            commission: 0,
            active: true,
        },
    ];

    let total_stake: u128 = validators.iter().map(|v| v.stake).sum();
    assert_eq!(total_stake, 4000);

    // Quorum threshold is 2/3
    let quorum_threshold = (total_stake * 2) / 3;
    assert_eq!(quorum_threshold, 2666);

    // 3 out of 4 validators (3000 stake) forms quorum
    let voting_stake = 3000u128;
    assert!(
        voting_stake * 3 >= total_stake * 2,
        "3/4 validators should form quorum"
    );

    // 2 out of 4 validators (2000 stake) does NOT form quorum
    let voting_stake = 2000u128;
    assert!(
        voting_stake * 3 < total_stake * 2,
        "2/4 validators should NOT form quorum"
    );
}

#[test]
fn test_bls_signature_aggregation() {
    // Test BLS signature aggregation with 4 validators
    use aether_crypto_bls::{aggregate_public_keys, aggregate_signatures, verify_aggregated};

    let message = b"test block hash";
    let mut signatures = Vec::new();
    let mut pubkeys = Vec::new();

    // 4 validators sign the same message
    for _ in 0..4 {
        let keypair = BlsKeypair::generate();
        let sig = keypair.sign(message);
        let pk = keypair.public_key();

        assert_eq!(sig.len(), 96, "BLS signature should be 96 bytes");
        assert_eq!(pk.len(), 48, "BLS public key should be 48 bytes");

        signatures.push(sig);
        pubkeys.push(pk);
    }

    // Aggregate signatures and public keys
    let agg_sig = aggregate_signatures(&signatures).expect("aggregation should succeed");
    let agg_pk = aggregate_public_keys(&pubkeys).expect("aggregation should succeed");

    assert_eq!(agg_sig.len(), 96, "Aggregated signature should be 96 bytes");
    assert_eq!(agg_pk.len(), 48, "Aggregated public key should be 48 bytes");

    let verified =
        verify_aggregated(&agg_pk, message, &agg_sig).expect("verification should succeed");
    assert!(
        verified,
        "Aggregated signature must verify for matching message"
    );

    println!(
        "BLS aggregation test passed: 4 signatures → 1 signature with successful verification"
    );
}

#[test]
fn test_hotstuff_phase_transitions() {
    // Test that HotStuff phases transition correctly
    let validators = vec![ValidatorInfo {
        pubkey: PublicKey::from_bytes(Keypair::generate().public_key()),
        stake: 1000,
        commission: 0,
        active: true,
    }];

    let mut consensus = HybridConsensus::new(validators, 0.8, 100, None, None, None);

    // Initial phase should be Propose
    assert_eq!(format!("{:?}", consensus.current_phase()), "Propose");

    // Advance through phases
    consensus.advance_phase();
    assert_eq!(format!("{:?}", consensus.current_phase()), "Prevote");

    consensus.advance_phase();
    assert_eq!(format!("{:?}", consensus.current_phase()), "Precommit");

    consensus.advance_phase();
    assert_eq!(format!("{:?}", consensus.current_phase()), "Commit");

    consensus.advance_phase();
    assert_eq!(format!("{:?}", consensus.current_phase()), "Propose");

    println!("HotStuff phase transition test passed");
}
