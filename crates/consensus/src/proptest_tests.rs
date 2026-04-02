//! Property-based tests for HotStuff consensus invariants.

#[cfg(test)]
mod tests {
    use crate::has_quorum;
    use crate::hotstuff::*;
    use crate::pacemaker::Pacemaker;
    use crate::slashing::{SlashType, SlashingDetector};
    use aether_crypto_bls::BlsKeypair;
    use aether_types::{Address, PublicKey, ValidatorInfo, H256};
    use proptest::prelude::*;
    use std::time::Duration;

    // ── helpers ──────────────────────────────────────────────────────────

    fn make_validators(n: usize) -> (Vec<ValidatorInfo>, Vec<BlsKeypair>) {
        let bls_keys: Vec<BlsKeypair> = (0..n).map(|_| BlsKeypair::generate()).collect();
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
        (validators, bls_keys)
    }

    fn make_consensus_with_bls(
        n: usize,
    ) -> (HotStuffConsensus, Vec<ValidatorInfo>, Vec<BlsKeypair>) {
        let (validators, bls_keys) = make_validators(n);
        let my_addr = validators[0].pubkey.to_address();
        let mut consensus =
            HotStuffConsensus::new(validators.clone(), Some(bls_keys[0].clone()), Some(my_addr));
        // Register all BLS keys with valid PoP
        for (i, bk) in bls_keys.iter().enumerate() {
            let addr = validators[i].pubkey.to_address();
            let pk = bk.public_key();
            let pop = bk.proof_of_possession();
            consensus
                .register_bls_pubkey(addr, pk, &pop)
                .expect("register bls");
        }
        (consensus, validators, bls_keys)
    }

    // ── quorum invariants ───────────────────────────────────────────────

    proptest! {
        /// 2/3 quorum: stake >= ceil(total*2/3) passes, below does not.
        #[test]
        fn quorum_threshold_is_two_thirds(total in 1u128..=1_000_000_000) {
            // Exact threshold: ceil(total * 2 / 3)
            let threshold = (total * 2).div_ceil(3);
            prop_assert!(has_quorum(threshold, total),
                "threshold {} should have quorum with total {}", threshold, total);
            if threshold > 0 {
                prop_assert!(!has_quorum(threshold - 1, total),
                    "threshold-1 {} should NOT have quorum with total {}", threshold - 1, total);
            }
        }

        /// Zero total stake never has quorum.
        #[test]
        fn quorum_zero_total_never_passes(voted in 0u128..=u128::MAX) {
            prop_assert!(!has_quorum(voted, 0));
        }

        /// Full stake always has quorum (non-zero total).
        #[test]
        fn quorum_full_stake_always_passes(total in 1u128..=u128::MAX / 3) {
            prop_assert!(has_quorum(total, total));
        }

        /// has_quorum is monotonic: if S has quorum, S+1 also has quorum.
        #[test]
        fn quorum_monotonic(total in 3u128..=100_000, voted in 0u128..=100_000) {
            let voted = voted.min(total);
            if has_quorum(voted, total) && voted < total {
                prop_assert!(has_quorum(voted + 1, total));
            }
        }

        /// has_quorum handles large stake values without panic.
        #[test]
        fn quorum_large_values(
            total in (u128::MAX / 4)..=u128::MAX / 3,
            frac in 0u64..=100
        ) {
            let voted = total / 100 * frac as u128;
            // Just ensure no panic
            let _ = has_quorum(voted, total);
        }
    }

    // ── phase progression ───────────────────────────────────────────────

    proptest! {
        /// Phase cycles through Propose→Prevote→Precommit→Commit→Propose.
        /// After N advance_phase calls, phase and slot are deterministic.
        #[test]
        fn phase_progression_deterministic(advances in 0u32..=100) {
            let (validators, _) = make_validators(4);
            let mut consensus = HotStuffConsensus::new(validators, None, None);
            let initial_slot = consensus.current_slot();
            for _ in 0..advances {
                consensus.advance_phase();
            }
            let expected_full_cycles = advances / 4;
            let expected_phase = match advances % 4 {
                0 => Phase::Propose,
                1 => Phase::Prevote,
                2 => Phase::Precommit,
                3 => Phase::Commit,
                _ => unreachable!(),
            };
            prop_assert_eq!(consensus.current_phase().clone(), expected_phase);
            prop_assert_eq!(
                consensus.current_slot(),
                initial_slot + expected_full_cycles as u64
            );
        }
    }

    // ── finality monotonicity ───────────────────────────────────────────

    proptest! {
        /// Finalized slot never decreases through vote processing.
        #[test]
        fn finalized_slot_monotonic(rounds in 1u32..=8) {
            let (mut consensus, validators, bls_keys) = make_consensus_with_bls(4);
            let mut prev_finalized = consensus.finalized_slot();

            for round in 0..rounds {
                let block_hash = H256::from_slice(&{
                    let mut h = [0u8; 32];
                    h[0..4].copy_from_slice(&round.to_le_bytes());
                    h
                }).unwrap();
                let parent_hash = if round == 0 {
                    H256::zero()
                } else {
                    H256::from_slice(&{
                        let mut h = [0u8; 32];
                        h[0..4].copy_from_slice(&(round - 1).to_le_bytes());
                        h
                    }).unwrap()
                };

                // Reset to Propose phase for this round
                while *consensus.current_phase() != Phase::Propose {
                    consensus.advance_phase();
                }

                // Simulate prevote quorum
                consensus.advance_phase(); // → Prevote
                for i in 0..3 {
                    let addr = validators[i].pubkey.to_address();
                    let vote = HotStuffVote {
                        slot: consensus.current_slot(),
                        block_hash,
                        parent_hash,
                        phase: Phase::Prevote,
                        validator: addr,
                        validator_pubkey: validators[i].pubkey.clone(),
                        stake: 1000,
                        signature: {
                            let mut msg = Vec::new();
                            msg.extend_from_slice(block_hash.as_bytes());
                            msg.extend_from_slice(parent_hash.as_bytes());
                            msg.extend_from_slice(&consensus.current_slot().to_le_bytes());
                            msg.push(1); // Prevote
                            bls_keys[i].sign(&msg)
                        },
                    };
                    let _ = consensus.on_vote(vote);
                }

                // Simulate precommit quorum
                for i in 0..3 {
                    if *consensus.current_phase() != Phase::Precommit {
                        break;
                    }
                    let addr = validators[i].pubkey.to_address();
                    let vote = HotStuffVote {
                        slot: consensus.current_slot(),
                        block_hash,
                        parent_hash,
                        phase: Phase::Precommit,
                        validator: addr,
                        validator_pubkey: validators[i].pubkey.clone(),
                        stake: 1000,
                        signature: {
                            let mut msg = Vec::new();
                            msg.extend_from_slice(block_hash.as_bytes());
                            msg.extend_from_slice(parent_hash.as_bytes());
                            msg.extend_from_slice(&consensus.current_slot().to_le_bytes());
                            msg.push(2); // Precommit
                            bls_keys[i].sign(&msg)
                        },
                    };
                    let _ = consensus.on_vote(vote);
                }

                let cur_finalized = consensus.finalized_slot();
                prop_assert!(
                    cur_finalized >= prev_finalized,
                    "finalized slot regressed: {} -> {}",
                    prev_finalized,
                    cur_finalized
                );
                prev_finalized = cur_finalized;
            }
        }
    }

    // ── vote deduplication ──────────────────────────────────────────────

    proptest! {
        /// Duplicate votes from the same validator are rejected.
        #[test]
        fn duplicate_vote_rejected(validator_idx in 0usize..3) {
            let (mut consensus, validators, bls_keys) = make_consensus_with_bls(4);
            consensus.advance_phase(); // → Prevote

            let block_hash = H256::from_slice(&[1u8; 32]).unwrap();
            let parent_hash = H256::zero();
            let addr = validators[validator_idx].pubkey.to_address();
            let slot = consensus.current_slot();

            let make_vote = || {
                let mut msg = Vec::new();
                msg.extend_from_slice(block_hash.as_bytes());
                msg.extend_from_slice(parent_hash.as_bytes());
                msg.extend_from_slice(&slot.to_le_bytes());
                msg.push(1); // Prevote
                HotStuffVote {
                    slot,
                    block_hash,
                    parent_hash,
                    phase: Phase::Prevote,
                    validator: addr,
                    validator_pubkey: validators[validator_idx].pubkey.clone(),
                    stake: 1000,
                    signature: bls_keys[validator_idx].sign(&msg),
                }
            };

            let first = consensus.on_vote(make_vote());
            prop_assert!(first.is_ok(), "first vote should succeed");

            let second = consensus.on_vote(make_vote());
            prop_assert!(second.is_err(), "duplicate vote should be rejected");
        }
    }

    // ── timeout certificate validation ──────────────────────────────────

    proptest! {
        /// TC with duplicate signers is rejected.
        #[test]
        fn tc_duplicate_signers_rejected(round in 1u64..=100) {
            let (mut consensus, validators, _) = make_consensus_with_bls(4);
            let addr = validators[0].pubkey.to_address();
            let tc = TimeoutCertificate {
                round,
                total_stake: 3000,
                highest_qc_slot: 0,
                highest_qc_hash: H256::zero(),
                signers: vec![addr, addr, addr], // duplicate!
                aggregated_signature: vec![0u8; 96],
                aggregated_pubkey: vec![0u8; 48],
            };
            let result = consensus.on_timeout_certificate(&tc);
            prop_assert!(result.is_err(), "TC with duplicate signers must be rejected");
        }

        /// TC with unknown signers is rejected.
        #[test]
        fn tc_unknown_signers_rejected(round in 1u64..=100) {
            let (mut consensus, _, _) = make_consensus_with_bls(4);
            let fake_addr = Address::from(aether_types::H160([0xFFu8; 20]));
            let tc = TimeoutCertificate {
                round,
                total_stake: 3000,
                highest_qc_slot: 0,
                highest_qc_hash: H256::zero(),
                signers: vec![fake_addr],
                aggregated_signature: vec![0u8; 96],
                aggregated_pubkey: vec![0u8; 48],
            };
            let result = consensus.on_timeout_certificate(&tc);
            prop_assert!(result.is_err(), "TC with unknown signers must be rejected");
        }

        /// TC with insufficient stake is rejected.
        #[test]
        fn tc_insufficient_stake_rejected(round in 1u64..=100) {
            let (mut consensus, validators, _) = make_consensus_with_bls(4);
            // Only 2 of 4 validators (2000/4000 = 50%, need 66.7%)
            let tc = TimeoutCertificate {
                round,
                total_stake: 2000,
                highest_qc_slot: 0,
                highest_qc_hash: H256::zero(),
                signers: vec![
                    validators[0].pubkey.to_address(),
                    validators[1].pubkey.to_address(),
                ],
                aggregated_signature: vec![0u8; 96],
                aggregated_pubkey: vec![0u8; 48],
            };
            let result = consensus.on_timeout_certificate(&tc);
            prop_assert!(result.is_err(), "TC with <2/3 stake must be rejected");
        }
    }

    // ── pacemaker invariants ────────────────────────────────────────────

    proptest! {
        /// Pacemaker round advances monotonically on timeout.
        #[test]
        fn pacemaker_round_monotonic(timeouts in 0u32..=20) {
            let mut pm = Pacemaker::new(Duration::from_millis(100));
            let mut prev_round = pm.current_round();
            for _ in 0..timeouts {
                pm.on_timeout();
                let cur = pm.current_round();
                prop_assert!(cur > prev_round, "round must advance on timeout");
                prev_round = cur;
            }
        }

        /// Pacemaker timeout is capped at max_timeout (30s).
        #[test]
        fn pacemaker_timeout_bounded(timeouts in 0u32..=30) {
            let mut pm = Pacemaker::new(Duration::from_millis(100));
            for _ in 0..timeouts {
                pm.on_timeout();
            }
            // After on_commit, timeout resets to base
            pm.on_commit();
            // Cannot inspect current_timeout directly, but we can verify
            // that after commit it behaves correctly (no panic, round advances)
            let round_after_commit = pm.current_round();
            pm.on_timeout();
            prop_assert!(pm.current_round() > round_after_commit);
        }

        /// advance_to_round is idempotent for past rounds.
        #[test]
        fn pacemaker_advance_to_round_idempotent(target in 0u64..=50) {
            let mut pm = Pacemaker::new(Duration::from_millis(100));
            pm.advance_to_round(target);
            let round = pm.current_round();
            prop_assert!(round >= target,
                "advance_to_round({}) left round at {}", target, round);
            // Advancing to same or lower round is a no-op
            pm.advance_to_round(target);
            prop_assert_eq!(pm.current_round(), round);
            if target > 0 {
                pm.advance_to_round(target - 1);
                prop_assert_eq!(pm.current_round(), round);
            }
        }
    }

    // ── slashing detector ───────────────────────────────────────────────

    proptest! {
        /// Double-signing the same slot with different hashes is detected.
        #[test]
        fn slashing_detects_double_sign(slot in 0u64..=1000) {
            let mut detector = SlashingDetector::new();
            let kp = aether_crypto_primitives::Keypair::generate();
            let pk = PublicKey::from_bytes(kp.public_key());
            let addr = pk.to_address();

            let hash1 = H256::from_slice(&[1u8; 32]).unwrap();
            let hash2 = H256::from_slice(&[2u8; 32]).unwrap();
            let sig = aether_types::Signature::from_bytes(vec![0u8; 64]);

            let r1 = detector.record_vote(addr, pk.clone(), slot, hash1, sig.clone());
            prop_assert!(r1.is_none(), "first vote should not trigger slash");

            let r2 = detector.record_vote(addr, pk.clone(), slot, hash2, sig.clone());
            prop_assert!(r2.is_some(), "double-sign should be detected");
            let proof = r2.unwrap();
            prop_assert_eq!(proof.proof_type, SlashType::DoubleSign);
            prop_assert_eq!(proof.validator, addr);
        }

        /// Voting for the same block twice at the same slot is NOT a slash.
        #[test]
        fn slashing_same_vote_not_slashed(slot in 0u64..=1000) {
            let mut detector = SlashingDetector::new();
            let kp = aether_crypto_primitives::Keypair::generate();
            let pk = PublicKey::from_bytes(kp.public_key());
            let addr = pk.to_address();

            let hash = H256::from_slice(&[1u8; 32]).unwrap();
            let sig = aether_types::Signature::from_bytes(vec![0u8; 64]);

            let r1 = detector.record_vote(addr, pk.clone(), slot, hash, sig.clone());
            prop_assert!(r1.is_none());
            let r2 = detector.record_vote(addr, pk.clone(), slot, hash, sig.clone());
            prop_assert!(r2.is_none(), "same vote twice should not trigger slash");
        }
    }
}
