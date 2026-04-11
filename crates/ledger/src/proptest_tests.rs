// ============================================================================
// AETHER LEDGER — Property-based tests
// ============================================================================
// Tests that generate random valid/invalid transactions and verify the ledger
// accepts/rejects them correctly.  Run with:
//   cargo test --package aether-ledger -- proptest
// or (for the ignored, longer runs):
//   cargo test --package aether-ledger --features proptest -- --ignored
// ============================================================================

use crate::state::Ledger;
use aether_crypto_primitives::Keypair;
use aether_state_storage::Storage;
use aether_types::{
    Address, PublicKey, Signature, Transaction, TransactionStatus, TransferPayload,
    TRANSFER_PROGRAM_ID,
};
use proptest::prelude::*;
use std::collections::HashSet;
use tempfile::TempDir;

// ── helpers ──────────────────────────────────────────────────────────────────

fn open_ledger() -> (TempDir, Ledger) {
    let dir = TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let ledger = Ledger::new(storage).unwrap();
    (dir, ledger)
}

fn seed(ledger: &mut Ledger, address: &Address, balance: u128) {
    ledger.seed_account(address, balance).unwrap();
}

/// Build a correctly-signed transfer transaction.
fn make_transfer(
    kp: &Keypair,
    sender: Address,
    recipient: Address,
    amount: u128,
    fee: u128,
    nonce: u64,
    chain_id: u64,
) -> Transaction {
    let payload = TransferPayload {
        recipient,
        amount,
        memo: None,
    };
    let mut tx = Transaction {
        nonce,
        chain_id,
        sender,
        sender_pubkey: PublicKey::from_bytes(kp.public_key()),
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: Some(TRANSFER_PROGRAM_ID),
        data: bincode::serialize(&payload).unwrap(),
        gas_limit: 21_000,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(kp.sign(hash.as_bytes()));
    tx
}

// ── proptest strategies ───────────────────────────────────────────────────────

/// Strategy: transfer amounts in [1, 10_000]
fn arb_amount() -> impl Strategy<Value = u128> {
    1u128..=10_000u128
}

/// Strategy: fee in [0, 500]
fn arb_fee() -> impl Strategy<Value = u128> {
    0u128..=500u128
}

// ── property tests ────────────────────────────────────────────────────────────

proptest! {
    /// Any valid transfer where sender has sufficient balance succeeds.
    #[test]
    fn prop_valid_transfer_succeeds(
        amount in arb_amount(),
        fee in arb_fee(),
    ) {
        let sender_kp = Keypair::generate();
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, amount + fee + 1_000); // always enough

        let tx = make_transfer(&sender_kp, sender, recipient, amount, fee, 0, 1);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(
            matches!(receipt.status, TransactionStatus::Success),
            "expected success, got {:?}",
            receipt.status
        );

        // Recipient balance increases by `amount`.
        let rec_acc = ledger.get_or_create_account(&recipient).unwrap();
        prop_assert_eq!(rec_acc.balance, amount);
    }

    /// Transfer where sender has insufficient balance fails with the ledger
    /// returning a Failed receipt (not an Err).
    #[test]
    fn prop_insufficient_balance_fails(
        amount in 2u128..=10_000u128,
        fee in 0u128..=100u128,
    ) {
        let sender_kp = Keypair::generate();
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        // Fund with strictly less than required.
        let balance = amount.saturating_sub(1);
        seed(&mut ledger, &sender, balance);

        let tx = make_transfer(&sender_kp, sender, recipient, amount, fee, 0, 1);
        // apply_transaction either returns an Err or a Failed receipt.
        if let Ok(receipt) = ledger.apply_transaction(&tx) {
            prop_assert!(
                matches!(receipt.status, TransactionStatus::Failed { .. }),
                "expected Failed receipt, got Success"
            );
        }
    }

    /// A transaction with a bad signature is always rejected.
    #[test]
    fn prop_bad_signature_rejected(
        amount in arb_amount(),
        fee in arb_fee(),
    ) {
        let sender_kp = Keypair::generate();
        let attacker_kp = Keypair::generate(); // different key for signing
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, amount + fee + 1_000);

        // Build tx but sign with attacker's key.
        let payload = TransferPayload { recipient, amount, memo: None };
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender,
            sender_pubkey: PublicKey::from_bytes(sender_kp.public_key()),
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: Some(TRANSFER_PROGRAM_ID),
            data: bincode::serialize(&payload).unwrap(),
            gas_limit: 21_000,
            fee,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        // Sign with attacker key — mismatches sender_pubkey
        tx.signature = Signature::from_bytes(attacker_kp.sign(hash.as_bytes()));

        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err(), "bad signature must be rejected, got Ok");
    }

    /// Sequential nonces [0, 1, 2, …] all succeed (no gaps).
    #[test]
    fn prop_sequential_nonces_all_succeed(count in 1usize..=8usize) {
        let sender_kp = Keypair::generate();
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        // Fund enough for `count` transfers of 10 + fee 10 each.
        seed(&mut ledger, &sender, (count as u128) * 200 + 1_000);

        for nonce in 0..count {
            let tx = make_transfer(&sender_kp, sender, recipient, 10, 10, nonce as u64, 1);
            let receipt = ledger.apply_transaction(&tx).unwrap();
            prop_assert!(
                matches!(receipt.status, TransactionStatus::Success),
                "tx with nonce {} failed: {:?}", nonce, receipt.status
            );
        }
    }

    /// Replaying the same nonce is always rejected.
    #[test]
    fn prop_nonce_replay_rejected(
        amount in arb_amount(),
        fee in arb_fee(),
    ) {
        let sender_kp = Keypair::generate();
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, (amount + fee) * 3 + 1_000);

        let tx = make_transfer(&sender_kp, sender, recipient, amount, fee, 0, 1);
        ledger.apply_transaction(&tx).unwrap(); // first time: ok
        let result = ledger.apply_transaction(&tx); // replay: must fail
        if let Ok(receipt) = result {
            prop_assert!(
                matches!(receipt.status, TransactionStatus::Failed { .. }),
                "replay must be rejected"
            );
        }
    }

    /// Skipping a nonce (e.g., using nonce 1 before nonce 0) is rejected.
    #[test]
    fn prop_skipped_nonce_rejected(
        amount in arb_amount(),
        fee in arb_fee(),
        skip in 1u64..=10u64,
    ) {
        let sender_kp = Keypair::generate();
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, amount + fee + 1_000);

        // Use nonce = skip (skipping 0..skip-1)
        let tx = make_transfer(&sender_kp, sender, recipient, amount, fee, skip, 1);
        if let Ok(receipt) = ledger.apply_transaction(&tx) {
            prop_assert!(
                matches!(receipt.status, TransactionStatus::Failed { .. }),
                "skipped nonce must be rejected"
            );
        }
    }

    /// Wrong chain_id is always rejected.
    #[test]
    fn prop_wrong_chain_id_rejected(
        amount in arb_amount(),
        fee in arb_fee(),
        bad_chain_id in 2u64..=1000u64, // chain_id 1 is the correct one
    ) {
        let sender_kp = Keypair::generate();
        let recipient_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, amount + fee + 1_000);

        let tx = make_transfer(&sender_kp, sender, recipient, amount, fee, 0, bad_chain_id);
        let (receipts, _overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[tx], Some(1))
            .unwrap();
        prop_assert_eq!(receipts.len(), 1);
        prop_assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { reason } if reason.contains("chain_id")),
            "wrong chain_id must be rejected, got {:?}",
            receipts[0].status
        );
    }

    /// Zero-amount transfer to self never panics and either succeeds or fails
    /// gracefully (no panic is the key invariant).
    #[test]
    fn prop_self_transfer_no_panic(balance in 0u128..=100_000u128) {
        let sender_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, balance);

        // Transfer 0 to self — should not panic regardless of balance.
        let tx = make_transfer(&sender_kp, sender, sender, 0, 0, 0, 1);
        let _ = ledger.apply_transaction(&tx); // outcome doesn't matter — must not panic
    }
}

// ── deterministic regression tests (always run, not proptest) ────────────────

#[test]
fn test_balance_preserved_after_transfer() {
    let sender_kp = Keypair::generate();
    let recipient_kp = Keypair::generate();
    let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
    let recipient = Address::from_slice(&recipient_kp.to_address()).unwrap();

    let (_dir, mut ledger) = open_ledger();
    let initial = 5_000u128;
    let amount = 1_000u128;
    let fee = 100u128;
    seed(&mut ledger, &sender, initial);

    let tx = make_transfer(&sender_kp, sender, recipient, amount, fee, 0, 1);
    let receipt = ledger.apply_transaction(&tx).unwrap();
    assert!(matches!(receipt.status, TransactionStatus::Success));

    let sender_acc = ledger.get_or_create_account(&sender).unwrap();
    let recip_acc = ledger.get_or_create_account(&recipient).unwrap();

    // Conservation: sender loses amount+fee (fee partially burned/proposer),
    // recipient gains amount.
    assert_eq!(recip_acc.balance, amount);
    assert!(sender_acc.balance <= initial - amount - fee);
}

#[test]
fn test_speculative_block_multi_sender_isolation() {
    // Two senders, each with exactly enough for one tx.  Both txs in one block
    // must succeed independently.
    let kp1 = Keypair::generate();
    let kp2 = Keypair::generate();
    let addr1 = Address::from_slice(&kp1.to_address()).unwrap();
    let addr2 = Address::from_slice(&kp2.to_address()).unwrap();
    let recip_kp = Keypair::generate();
    let recip = Address::from_slice(&recip_kp.to_address()).unwrap();

    let (_dir, mut ledger) = open_ledger();
    seed(&mut ledger, &addr1, 500);
    seed(&mut ledger, &addr2, 500);

    let tx1 = make_transfer(&kp1, addr1, recip, 100, 50, 0, 1);
    let tx2 = make_transfer(&kp2, addr2, recip, 200, 50, 0, 1);

    let (receipts, overlay) = ledger
        .apply_block_speculatively_with_chain_id(&[tx1, tx2], Some(1))
        .unwrap();

    assert_eq!(receipts.len(), 2);
    for r in &receipts {
        assert!(
            matches!(r.status, TransactionStatus::Success),
            "expected Success, got {:?}",
            r.status
        );
    }

    // Commit and verify final state.
    ledger.commit_overlay(overlay).unwrap();
    let recip_acc = ledger.get_or_create_account(&recip).unwrap();
    assert_eq!(recip_acc.balance, 300, "recipient should have 100+200");
}

// ── speculative block isolation ────────────────────────────────────────────
//
// These proptests verify that failed transactions inside a speculative block
// do not corrupt the overlay state visible to subsequent transactions.
// This is the core isolation invariant: if tx[i] fails, tx[i+1] must see
// the same overlay state as if tx[i] never existed.

proptest! {
    /// A failed transaction (bad nonce) in a speculative block must not
    /// prevent a subsequent valid transaction from the same sender.
    ///
    /// Scenario: sender submits [tx_nonce=5 (wrong), tx_nonce=0 (correct)].
    /// tx[0] must fail, tx[1] must succeed. If the overlay leaks the failed
    /// tx's nonce increment, tx[1] would see nonce=1 and also fail.
    #[test]
    fn prop_failed_nonce_does_not_leak_in_overlay(
        balance in 1_000u128..=100_000u128,
        bad_nonce in 1u64..=100u64,
    ) {
        let sender_kp = Keypair::generate();
        let recip_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recip = Address::from_slice(&recip_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, balance);

        let bad_tx = make_transfer(&sender_kp, sender, recip, 10, 0, bad_nonce, 1);
        let good_tx = make_transfer(&sender_kp, sender, recip, 10, 0, 0, 1);

        let (receipts, _overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[bad_tx, good_tx], Some(1))
            .unwrap();

        prop_assert_eq!(receipts.len(), 2);
        prop_assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { reason } if reason.contains("nonce")),
            "bad nonce tx should fail, got {:?}", receipts[0].status
        );
        prop_assert!(
            matches!(receipts[1].status, TransactionStatus::Success),
            "valid tx after failed tx should succeed, got {:?}", receipts[1].status
        );
    }

    /// A failed transaction (insufficient balance) must not affect the
    /// balance visible to subsequent transactions from a different sender.
    #[test]
    fn prop_failed_balance_does_not_corrupt_other_senders(
        amount1 in 100u128..=10_000u128,
        amount2 in 100u128..=10_000u128,
    ) {
        let kp1 = Keypair::generate();
        let kp2 = Keypair::generate();
        let recip_kp = Keypair::generate();
        let addr1 = Address::from_slice(&kp1.to_address()).unwrap();
        let addr2 = Address::from_slice(&kp2.to_address()).unwrap();
        let recip = Address::from_slice(&recip_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &addr1, 1); // too little — tx1 will fail
        seed(&mut ledger, &addr2, amount2 + 1_000);

        let tx1 = make_transfer(&kp1, addr1, recip, amount1, 0, 0, 1);
        let tx2 = make_transfer(&kp2, addr2, recip, amount2, 0, 0, 1);

        let (receipts, overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[tx1, tx2], Some(1))
            .unwrap();

        prop_assert_eq!(receipts.len(), 2);
        prop_assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { .. }),
            "underfunded tx should fail, got {:?}", receipts[0].status
        );
        prop_assert!(
            matches!(receipts[1].status, TransactionStatus::Success),
            "funded tx should succeed regardless of prior failure, got {:?}", receipts[1].status
        );

        ledger.commit_overlay(overlay).unwrap();
        let recip_acc = ledger.get_or_create_account(&recip).unwrap();
        prop_assert_eq!(
            recip_acc.balance, amount2,
            "recipient should only receive amount2 from the successful tx"
        );
    }

    /// Sequential valid transactions from the same sender in one speculative
    /// block must each see the prior tx's nonce and balance updates.
    #[test]
    fn prop_sequential_same_sender_in_block(count in 2usize..=6) {
        let sender_kp = Keypair::generate();
        let recip_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recip = Address::from_slice(&recip_kp.to_address()).unwrap();

        let per_tx = 100u128;
        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, per_tx * (count as u128) + 10_000);

        let txs: Vec<Transaction> = (0..count)
            .map(|i| make_transfer(&sender_kp, sender, recip, per_tx, 0, i as u64, 1))
            .collect();

        let (receipts, overlay) = ledger
            .apply_block_speculatively_with_chain_id(&txs, Some(1))
            .unwrap();

        prop_assert_eq!(receipts.len(), count);
        for (i, r) in receipts.iter().enumerate() {
            prop_assert!(
                matches!(r.status, TransactionStatus::Success),
                "tx[{}] (nonce {}) should succeed, got {:?}", i, i, r.status
            );
        }

        ledger.commit_overlay(overlay).unwrap();
        let recip_acc = ledger.get_or_create_account(&recip).unwrap();
        prop_assert_eq!(recip_acc.balance, per_tx * (count as u128));
        let sender_acc = ledger.get_or_create_account(&sender).unwrap();
        prop_assert_eq!(sender_acc.nonce, count as u64);
    }

    /// Interleaving valid and invalid txs from the same sender: valid txs
    /// must execute correctly while invalid ones produce Failed receipts
    /// without corrupting the nonce sequence.
    #[test]
    fn prop_interleaved_valid_invalid_same_sender(
        balance in 500u128..=50_000u128,
    ) {
        let sender_kp = Keypair::generate();
        let recip_kp = Keypair::generate();
        let sender = Address::from_slice(&sender_kp.to_address()).unwrap();
        let recip = Address::from_slice(&recip_kp.to_address()).unwrap();

        let (_dir, mut ledger) = open_ledger();
        seed(&mut ledger, &sender, balance);

        // tx0: nonce=99 (WRONG) → fail, overlay untouched
        // tx1: nonce=0  (correct) → succeed, nonce becomes 1
        // tx2: nonce=0  (replay) → fail, nonce still 1
        // tx3: nonce=1  (correct) → succeed, nonce becomes 2
        let tx0 = make_transfer(&sender_kp, sender, recip, 10, 0, 99, 1);
        let tx1 = make_transfer(&sender_kp, sender, recip, 10, 0, 0, 1);
        let tx2 = make_transfer(&sender_kp, sender, recip, 10, 0, 0, 1);
        let tx3 = make_transfer(&sender_kp, sender, recip, 10, 0, 1, 1);

        let (receipts, overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[tx0, tx1, tx2, tx3], Some(1))
            .unwrap();

        prop_assert_eq!(receipts.len(), 4);
        prop_assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { .. }),
            "tx0 (bad nonce) should fail: {:?}", receipts[0].status
        );
        prop_assert!(
            matches!(receipts[1].status, TransactionStatus::Success),
            "tx1 (nonce=0) should succeed: {:?}", receipts[1].status
        );
        prop_assert!(
            matches!(&receipts[2].status, TransactionStatus::Failed { .. }),
            "tx2 (replay nonce=0) should fail: {:?}", receipts[2].status
        );
        prop_assert!(
            matches!(receipts[3].status, TransactionStatus::Success),
            "tx3 (nonce=1) should succeed: {:?}", receipts[3].status
        );

        ledger.commit_overlay(overlay).unwrap();
        let sender_acc = ledger.get_or_create_account(&sender).unwrap();
        prop_assert_eq!(sender_acc.nonce, 2, "only 2 successful txs → nonce = 2");
        let recip_acc = ledger.get_or_create_account(&recip).unwrap();
        prop_assert_eq!(recip_acc.balance, 20, "only 2 successful 10-unit transfers");
    }

    /// Speculative execution and commit must produce the same state root as
    /// applying the same transactions individually via apply_transaction.
    #[test]
    fn prop_speculative_matches_committed_state(
        amount in 100u128..=5_000u128,
        fee in 0u128..=100u128,
    ) {
        let kp = Keypair::generate();
        let recip_kp = Keypair::generate();
        let sender = Address::from_slice(&kp.to_address()).unwrap();
        let recip = Address::from_slice(&recip_kp.to_address()).unwrap();

        let initial = amount + fee + 10_000;

        // Path A: speculative then commit
        let (dir_a, mut ledger_a) = open_ledger();
        seed(&mut ledger_a, &sender, initial);
        let tx = make_transfer(&kp, sender, recip, amount, fee, 0, 1);
        let (receipts, overlay) = ledger_a
            .apply_block_speculatively_with_chain_id(std::slice::from_ref(&tx), Some(1))
            .unwrap();
        prop_assert!(matches!(receipts[0].status, TransactionStatus::Success));
        let spec_root = overlay.state_root;
        ledger_a.commit_overlay(overlay).unwrap();
        let committed_root_a = ledger_a.state_root();
        drop(ledger_a);
        drop(dir_a);

        // Path B: direct apply_transaction
        let (_dir_b, mut ledger_b) = open_ledger();
        seed(&mut ledger_b, &sender, initial);
        let receipt = ledger_b.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));
        let committed_root_b = ledger_b.state_root();

        prop_assert_eq!(
            spec_root, committed_root_b,
            "speculative root must match direct-apply root"
        );
        prop_assert_eq!(
            committed_root_a, committed_root_b,
            "post-commit root must match direct-apply root"
        );
    }
}
