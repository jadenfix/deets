// Property-based tests for aether-types core types.
//
// Covers: serialization roundtrips (bincode), hash determinism,
// conflict symmetry, blob validation invariants, fee calculation safety.

use crate::primitives::*;
use crate::transaction::*;
use crate::account::*;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use std::collections::HashSet;

// ── Strategies ──────────────────────────────────────────────────────

fn arb_h256() -> impl Strategy<Value = H256> {
    proptest::array::uniform32(any::<u8>()).prop_map(H256)
}

fn arb_address() -> impl Strategy<Value = Address> {
    proptest::array::uniform20(any::<u8>()).prop_map(H160)
}

fn arb_pubkey() -> impl Strategy<Value = PublicKey> {
    proptest::collection::vec(any::<u8>(), 32..=32).prop_map(PublicKey::from_bytes)
}

fn arb_signature() -> impl Strategy<Value = Signature> {
    proptest::collection::vec(any::<u8>(), 0..=128).prop_map(Signature::from_bytes)
}

fn arb_utxo_id() -> impl Strategy<Value = UtxoId> {
    (arb_h256(), any::<u32>()).prop_map(|(tx_hash, output_index)| UtxoId {
        tx_hash,
        output_index,
    })
}

fn arb_utxo_output() -> impl Strategy<Value = UtxoOutput> {
    (any::<u128>(), arb_pubkey(), proptest::option::of(arb_h256())).prop_map(
        |(amount, owner, script_hash)| UtxoOutput {
            amount,
            owner,
            script_hash,
        },
    )
}

fn arb_transaction() -> impl Strategy<Value = Transaction> {
    (
        any::<u64>(),                                       // nonce
        any::<u64>(),                                       // chain_id
        arb_address(),                                      // sender
        arb_pubkey(),                                       // sender_pubkey
        proptest::collection::vec(arb_utxo_id(), 0..4),     // inputs
        proptest::collection::vec(arb_utxo_output(), 0..4), // outputs
        any::<u64>(),                                       // gas_limit
        any::<u128>(),                                      // fee
    )
        .prop_map(
            |(nonce, chain_id, sender, sender_pubkey, inputs, outputs, gas_limit, fee)| {
                Transaction {
                    nonce,
                    chain_id,
                    sender,
                    sender_pubkey,
                    inputs,
                    outputs,
                    reads: HashSet::new(),
                    writes: HashSet::new(),
                    program_id: None,
                    data: vec![],
                    gas_limit,
                    fee,
                    signature: Signature::from_bytes(vec![]),
                }
            },
        )
}

fn arb_account() -> impl Strategy<Value = Account> {
    (arb_address(), any::<u128>(), any::<u64>(), proptest::option::of(arb_h256()), arb_h256())
        .prop_map(|(address, balance, nonce, code_hash, storage_root)| Account {
            address,
            balance,
            nonce,
            code_hash,
            storage_root,
        })
}

fn arb_blob_tx(blob_count: u32) -> impl Strategy<Value = BlobTransaction> {
    let count = blob_count;
    (any::<u64>(), any::<u64>(), arb_address(), arb_pubkey(), any::<u64>(), any::<u128>())
        .prop_map(move |(nonce, chain_id, sender, sender_pubkey, gas_limit, fee)| {
            BlobTransaction {
                nonce,
                chain_id,
                sender,
                sender_pubkey,
                gas_limit,
                fee,
                signature: Signature::from_bytes(vec![0u8; 64]),
                blob_commitments: (0..count).map(|_| vec![0u8; 48]).collect(),
                blob_count: count,
                total_blob_size: (count as u64) * 1000,
                program_id: None,
                data: vec![],
            }
        })
}

// ── Serialization Roundtrip Tests ───────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn h256_bincode_roundtrip(hash in arb_h256()) {
        let bytes = bincode::serialize(&hash).unwrap();
        let decoded: H256 = bincode::deserialize(&bytes).unwrap();
        prop_assert_eq!(hash, decoded);
    }

    #[test]
    fn address_bincode_roundtrip(addr in arb_address()) {
        let bytes = bincode::serialize(&addr).unwrap();
        let decoded: Address = bincode::deserialize(&bytes).unwrap();
        prop_assert_eq!(addr, decoded);
    }

    #[test]
    fn signature_bincode_roundtrip(sig in arb_signature()) {
        let bytes = bincode::serialize(&sig).unwrap();
        let decoded: Signature = bincode::deserialize(&bytes).unwrap();
        prop_assert_eq!(sig, decoded);
    }

    #[test]
    fn pubkey_bincode_roundtrip(pk in arb_pubkey()) {
        let bytes = bincode::serialize(&pk).unwrap();
        let decoded: PublicKey = bincode::deserialize(&bytes).unwrap();
        prop_assert_eq!(pk, decoded);
    }

    #[test]
    fn transaction_bincode_roundtrip(tx in arb_transaction()) {
        let bytes = bincode::serialize(&tx).unwrap();
        let decoded: Transaction = bincode::deserialize(&bytes).unwrap();
        // Compare by re-serializing (Transaction doesn't derive PartialEq)
        let bytes2 = bincode::serialize(&decoded).unwrap();
        prop_assert_eq!(bytes, bytes2);
    }

    #[test]
    fn account_bincode_roundtrip(acc in arb_account()) {
        let bytes = bincode::serialize(&acc).unwrap();
        let decoded: Account = bincode::deserialize(&bytes).unwrap();
        let bytes2 = bincode::serialize(&decoded).unwrap();
        prop_assert_eq!(bytes, bytes2);
    }

    #[test]
    fn utxo_id_bincode_roundtrip(uid in arb_utxo_id()) {
        let bytes = bincode::serialize(&uid).unwrap();
        let decoded: UtxoId = bincode::deserialize(&bytes).unwrap();
        prop_assert_eq!(uid, decoded);
    }

    // ── Hash Determinism ────────────────────────────────────────────

    #[test]
    fn tx_hash_deterministic(tx in arb_transaction()) {
        let h1 = tx.hash();
        let h2 = tx.hash();
        prop_assert_eq!(h1, h2, "Transaction hash must be deterministic");
    }

    #[test]
    fn tx_hash_excludes_signature(tx in arb_transaction()) {
        let mut tx2 = tx.clone();
        tx2.signature = Signature::from_bytes(vec![99; 64]);
        prop_assert_eq!(tx.hash(), tx2.hash(), "Signature must not affect tx hash");
    }

    #[test]
    fn tx_different_nonce_different_hash(tx in arb_transaction()) {
        let mut tx2 = tx.clone();
        tx2.nonce = tx.nonce.wrapping_add(1);
        prop_assert_ne!(tx.hash(), tx2.hash(), "Different nonce must produce different hash");
    }

    #[test]
    fn tx_different_fee_different_hash(tx in arb_transaction()) {
        let mut tx2 = tx.clone();
        tx2.fee = tx.fee.wrapping_add(1);
        prop_assert_ne!(tx.hash(), tx2.hash(), "Different fee must produce different hash");
    }

    // ── Conflict Symmetry ───────────────────────────────────────────

    #[test]
    fn conflicts_with_is_symmetric(
        tx1 in arb_transaction(),
        tx2 in arb_transaction(),
    ) {
        prop_assert_eq!(
            tx1.conflicts_with(&tx2),
            tx2.conflicts_with(&tx1),
            "conflicts_with must be symmetric"
        );
    }

    #[test]
    fn tx_conflicts_with_self_when_has_inputs(tx in arb_transaction()) {
        if !tx.inputs.is_empty() {
            prop_assert!(tx.conflicts_with(&tx), "tx with inputs must conflict with itself");
        }
    }

    // ── Blob Validation ─────────────────────────────────────────────

    #[test]
    fn valid_blob_tx_passes(blob_count in 1u32..=MAX_BLOBS_PER_TX) {
        let strat = arb_blob_tx(blob_count);
        let mut runner = proptest::test_runner::TestRunner::default();
        let tx = strat.new_tree(&mut runner).unwrap().current();
        prop_assert!(tx.validate().is_ok(), "Valid blob tx should pass validation");
    }

    #[test]
    fn blob_tx_zero_count_fails(tx in arb_blob_tx(0)) {
        prop_assert!(tx.validate().is_err(), "Zero-blob tx must fail");
    }

    #[test]
    fn blob_tx_hash_deterministic(tx in arb_blob_tx(1)) {
        prop_assert_eq!(tx.hash(), tx.hash());
    }

    #[test]
    fn blob_tx_hash_excludes_signature(tx in arb_blob_tx(1)) {
        let mut tx2 = tx.clone();
        tx2.signature = Signature::from_bytes(vec![42; 64]);
        prop_assert_eq!(tx.hash(), tx2.hash());
    }

    // ── Fee Calculation Safety ──────────────────────────────────────

    #[test]
    fn blob_fee_never_panics(
        blob_count in 0u32..=u32::MAX,
        total_blob_size in 0u64..=u64::MAX,
        per_blob_fee in any::<u128>(),
        per_byte_fee in any::<u128>(),
    ) {
        let tx = BlobTransaction {
            nonce: 0,
            chain_id: 1,
            sender: Address::from_slice(&[0u8; 20]).unwrap(),
            sender_pubkey: PublicKey::from_bytes(vec![0u8; 32]),
            gas_limit: 0,
            fee: 0,
            signature: Signature::from_bytes(vec![]),
            blob_commitments: vec![],
            blob_count,
            total_blob_size,
            program_id: None,
            data: vec![],
        };
        let mut fee_params = crate::chain_config::ChainConfig::devnet().fees;
        fee_params.blob_per_blob_fee = per_blob_fee;
        fee_params.blob_per_byte_fee = per_byte_fee;
        // Must not panic — should saturate
        let _fee = tx.blob_fee(&fee_params);
    }

    // ── PublicKey → Address determinism ──────────────────────────────

    #[test]
    fn pubkey_to_address_deterministic(pk in arb_pubkey()) {
        prop_assert_eq!(pk.to_address(), pk.to_address());
    }

    // ── H256 / H160 from_slice ──────────────────────────────────────

    #[test]
    fn h256_from_slice_roundtrip(bytes in proptest::array::uniform32(any::<u8>())) {
        let h = H256::from_slice(&bytes).unwrap();
        prop_assert_eq!(h.as_bytes(), &bytes);
    }

    #[test]
    fn h256_from_slice_wrong_len(bytes in proptest::collection::vec(any::<u8>(), 0..100usize)) {
        if bytes.len() != 32 {
            prop_assert!(H256::from_slice(&bytes).is_err());
        }
    }

    #[test]
    fn h160_from_slice_wrong_len(bytes in proptest::collection::vec(any::<u8>(), 0..100usize)) {
        if bytes.len() != 20 {
            prop_assert!(H160::from_slice(&bytes).is_err());
        }
    }

    // ── Slot/Epoch arithmetic ───────────────────────────────────────

    #[test]
    fn slot_to_epoch_monotonic(slot1 in any::<u64>(), slot2 in any::<u64>(), epoch_slots in 1u64..=1_000_000) {
        let e1 = slot_to_epoch(slot1, epoch_slots);
        let e2 = slot_to_epoch(slot2, epoch_slots);
        if slot1 <= slot2 {
            prop_assert!(e1 <= e2);
        }
    }

    #[test]
    fn epoch_start_slot_no_panic(epoch in any::<u64>(), epoch_slots in any::<u64>()) {
        let _ = epoch_start_slot(epoch, epoch_slots); // must not panic (saturating)
    }
}
