use aether_crypto_bls::BlsKeypair;
use aether_light_client::header_store::HeaderStore;
use aether_light_client::state_query::{StateProof, StateQuery};
use aether_light_client::verifier::{FinalizedHeader, LightClientVerifier, ValidatorEntry};
use aether_state_merkle::SparseMerkleTree;
use aether_types::*;
use proptest::prelude::*;
use sha2::{Digest, Sha256};

// --- Helpers ---

fn make_validator(stake: u128) -> (BlsKeypair, ValidatorEntry) {
    let kp = BlsKeypair::generate();
    let entry = ValidatorEntry {
        pubkey: PublicKey::from_bytes(kp.public_key()),
        stake,
    };
    (kp, entry)
}

fn header_message(header: &BlockHeader) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(header.slot.to_le_bytes());
    hasher.update(header.parent_hash.as_bytes());
    hasher.update(header.state_root.as_bytes());
    hasher.update(header.transactions_root.as_bytes());
    hasher.update(header.receipts_root.as_bytes());
    hasher.finalize().to_vec()
}

fn make_header(slot: u64) -> BlockHeader {
    BlockHeader {
        version: 1,
        slot,
        parent_hash: H256::zero(),
        state_root: H256::from_slice(&[slot as u8; 32]).unwrap(),
        transactions_root: H256::zero(),
        receipts_root: H256::zero(),
        proposer: Address::from_slice(&[1u8; 20]).unwrap(),
        vrf_proof: VrfProof {
            output: [0u8; 32],
            proof: vec![0u8; 80],
        },
        timestamp: 1000 + slot,
    }
}

fn sign_header(
    header: &BlockHeader,
    keypairs: &[&BlsKeypair],
    entries: &[&ValidatorEntry],
) -> FinalizedHeader {
    let msg = header_message(header);
    let sigs: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.sign(&msg)).collect();
    let agg_sig = aether_crypto_bls::aggregate_signatures(&sigs).unwrap();
    let total_stake: u128 = entries.iter().map(|e| e.stake).sum();

    FinalizedHeader {
        header: header.clone(),
        aggregate_signature: agg_sig,
        signer_pubkeys: entries.iter().map(|e| e.pubkey.clone()).collect(),
        total_signing_stake: total_stake,
    }
}

// --- Verifier proptests ---

proptest! {
    /// Slot monotonicity: accepting a header at slot S means any header at slot <= S is rejected.
    #[test]
    fn slot_monotonicity(first_slot in 1u64..1000, delta in 0u64..100) {
        let (kp, entry) = make_validator(1000);
        let mut verifier = LightClientVerifier::new(vec![entry.clone()]);

        let h1 = make_header(first_slot);
        let fh1 = sign_header(&h1, &[&kp], &[&entry]);
        verifier.verify_finalized_header(&fh1).unwrap();

        prop_assert_eq!(verifier.finalized_slot(), first_slot);

        // Same or earlier slot must be rejected
        let regressed_slot = first_slot.saturating_sub(delta);
        let h2 = make_header(regressed_slot);
        let fh2 = sign_header(&h2, &[&kp], &[&entry]);
        prop_assert!(verifier.verify_finalized_header(&fh2).is_err());
    }

    /// Strictly increasing slots are always accepted (with valid signatures).
    #[test]
    fn strictly_increasing_slots_accepted(slots in prop::collection::vec(1u64..10000, 2..6)) {
        let (kp, entry) = make_validator(1000);
        let mut verifier = LightClientVerifier::new(vec![entry.clone()]);

        // Sort and dedup to get strictly increasing
        let mut sorted: Vec<u64> = slots;
        sorted.sort();
        sorted.dedup();
        if sorted.len() < 2 { return Ok(()); }

        for &slot in &sorted {
            let h = make_header(slot);
            let fh = sign_header(&h, &[&kp], &[&entry]);
            prop_assert!(verifier.verify_finalized_header(&fh).is_ok(),
                "slot {} should be accepted", slot);
        }

        prop_assert_eq!(verifier.finalized_slot(), *sorted.last().unwrap());
    }

    /// Quorum requires >= 2/3 stake. With 3 equal validators, 2 of 3 suffices, 1 of 3 does not.
    #[test]
    fn quorum_threshold(stake in 100u128..10000) {
        let vals: Vec<(BlsKeypair, ValidatorEntry)> =
            (0..3).map(|_| make_validator(stake)).collect();
        let entries: Vec<ValidatorEntry> = vals.iter().map(|(_, e)| e.clone()).collect();
        let mut verifier = LightClientVerifier::new(entries.clone());

        // 2/3 should pass
        let h = make_header(1);
        let fh = sign_header(
            &h,
            &[&vals[0].0, &vals[1].0],
            &[&vals[0].1, &vals[1].1],
        );
        prop_assert!(verifier.verify_finalized_header(&fh).is_ok());

        // 1/3 should fail
        let mut verifier2 = LightClientVerifier::new(entries);
        let h2 = make_header(1);
        let fh2 = sign_header(&h2, &[&vals[0].0], &[&vals[0].1]);
        prop_assert!(verifier2.verify_finalized_header(&fh2).is_err());
    }

    /// State root is updated after accepting a finalized header.
    #[test]
    fn state_root_tracks_accepted_header(slot in 1u64..1000) {
        let (kp, entry) = make_validator(1000);
        let mut verifier = LightClientVerifier::new(vec![entry.clone()]);

        let h = make_header(slot);
        let expected_root = h.state_root;
        let fh = sign_header(&h, &[&kp], &[&entry]);
        verifier.verify_finalized_header(&fh).unwrap();

        prop_assert_eq!(verifier.finalized_state_root(), expected_root);
    }

    /// Validator set update changes total stake and accepted signers.
    #[test]
    fn validator_set_update(stake1 in 100u128..5000, stake2 in 100u128..5000) {
        let (kp1, entry1) = make_validator(stake1);
        let mut verifier = LightClientVerifier::new(vec![entry1.clone()]);

        // Accept with original validator
        let h1 = make_header(1);
        let fh1 = sign_header(&h1, &[&kp1], &[&entry1]);
        verifier.verify_finalized_header(&fh1).unwrap();

        // Rotate to new validator set
        let (kp2, entry2) = make_validator(stake2);
        verifier.update_validators(vec![entry2.clone()]);

        // Old validator should be rejected (unknown signer)
        let h2 = make_header(2);
        let fh2 = sign_header(&h2, &[&kp1], &[&entry1]);
        prop_assert!(verifier.verify_finalized_header(&fh2).is_err());

        // New validator should be accepted
        let h3 = make_header(3);
        let fh3 = sign_header(&h3, &[&kp2], &[&entry2]);
        prop_assert!(verifier.verify_finalized_header(&fh3).is_ok());
    }
}

// --- HeaderStore proptests ---

proptest! {
    /// HeaderStore never exceeds max_headers capacity.
    #[test]
    fn header_store_bounded(
        max_cap in 1usize..20,
        num_inserts in 1usize..50,
    ) {
        let mut store = HeaderStore::new(max_cap);
        for i in 0..num_inserts {
            store.insert(make_header(i as u64));
        }
        prop_assert!(store.len() <= max_cap);
    }

    /// The latest header always has the highest slot.
    #[test]
    fn header_store_latest_is_highest(
        slots in prop::collection::vec(0u64..1000, 1..20),
    ) {
        let mut store = HeaderStore::new(100);
        let max_slot = *slots.iter().max().unwrap();
        for &slot in &slots {
            store.insert(make_header(slot));
        }
        prop_assert_eq!(store.latest().unwrap().slot, max_slot);
    }

    /// Eviction removes the oldest (lowest slot) headers.
    #[test]
    fn header_store_evicts_oldest(
        max_cap in 2usize..10,
        num_inserts in 10usize..30,
    ) {
        let mut store = HeaderStore::new(max_cap);
        for i in 0..num_inserts {
            store.insert(make_header(i as u64));
        }
        // Oldest surviving slot should be num_inserts - max_cap
        let oldest_expected = (num_inserts - max_cap) as u64;
        prop_assert!(store.get(oldest_expected).is_some(),
            "slot {} should survive", oldest_expected);
        if oldest_expected > 0 {
            prop_assert!(store.get(oldest_expected - 1).is_none(),
                "slot {} should be evicted", oldest_expected - 1);
        }
    }

    /// Inserting a duplicate slot replaces in-place without growing.
    #[test]
    fn header_store_duplicate_slot_no_growth(slot in 0u64..100) {
        let mut store = HeaderStore::new(10);
        store.insert(make_header(slot));
        let len_after_first = store.len();
        store.insert(make_header(slot));
        prop_assert_eq!(store.len(), len_after_first);
    }
}

// --- StateQuery proptests ---

proptest! {
    /// A valid inclusion proof verifies against the correct root.
    #[test]
    fn state_query_inclusion_roundtrip(addr_byte in 1u8..=255, val_byte in 1u8..=255) {
        let mut tree = SparseMerkleTree::new();
        let addr = Address::from_slice(&[addr_byte; 20]).unwrap();
        let value_hash = H256::from_slice(&[val_byte; 32]).unwrap();
        tree.update(addr, value_hash);

        let proof = tree.prove(&addr);
        let query = StateQuery::new(tree.root());
        let sp = StateProof {
            proof,
            value: Some(vec![val_byte]),
        };

        let result = query.verify_inclusion(&addr, &value_hash, &sp).unwrap();
        prop_assert!(result, "valid inclusion proof must verify");
    }

    /// An exclusion proof for a non-existent key verifies correctly.
    #[test]
    fn state_query_exclusion(
        present_byte in 1u8..=127,
        absent_byte in 128u8..=255u8,
    ) {
        let mut tree = SparseMerkleTree::new();
        let present = Address::from_slice(&[present_byte; 20]).unwrap();
        tree.update(present, H256::from_slice(&[1u8; 32]).unwrap());

        let absent = Address::from_slice(&[absent_byte; 20]).unwrap();
        let proof = tree.prove(&absent);
        let query = StateQuery::new(tree.root());
        let sp = StateProof { proof, value: None };

        let excluded = query.verify_exclusion(&absent, &sp).unwrap();
        prop_assert!(excluded, "absent key should have valid exclusion proof");
    }

    /// A proof against the wrong root is always rejected.
    #[test]
    fn state_query_wrong_root_rejected(addr_byte in 1u8..=255, wrong_byte in 1u8..=255) {
        let mut tree = SparseMerkleTree::new();
        let addr = Address::from_slice(&[addr_byte; 20]).unwrap();
        tree.update(addr, H256::from_slice(&[2u8; 32]).unwrap());
        let proof = tree.prove(&addr);

        let wrong_root = H256::from_slice(&[wrong_byte; 32]).unwrap();
        // Skip if wrong_root happens to match the real root
        prop_assume!(wrong_root != tree.root());

        let query = StateQuery::new(wrong_root);
        let sp = StateProof { proof, value: Some(vec![]) };
        prop_assert!(query.verify_account(&addr, &sp).is_err());
    }

    /// update_root changes what the query verifies against.
    #[test]
    fn state_query_root_update(byte1 in 1u8..=255, byte2 in 1u8..=255) {
        let root1 = H256::from_slice(&[byte1; 32]).unwrap();
        let root2 = H256::from_slice(&[byte2; 32]).unwrap();

        let mut query = StateQuery::new(root1);
        prop_assert_eq!(query.trusted_root(), root1);

        query.update_root(root2);
        prop_assert_eq!(query.trusted_root(), root2);
    }
}
