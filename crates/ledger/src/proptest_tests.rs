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
