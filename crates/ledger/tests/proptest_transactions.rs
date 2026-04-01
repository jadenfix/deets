//! Property-based tests for the Aether ledger transaction processing.
//!
//! These tests generate random valid and invalid transactions and verify
//! the ledger correctly accepts or rejects them.

use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_state_storage::Storage;
use aether_types::{
    Address, PublicKey, Signature, Transaction, TransactionStatus, TransferPayload,
    TRANSFER_PROGRAM_ID,
};
use proptest::prelude::*;
use std::collections::HashSet;
use tempfile::TempDir;

/// Helper: create a fresh ledger with a funded account (nonce starts at 0).
fn setup_ledger(address: &Address, balance: u128) -> (TempDir, Ledger) {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();
    ledger.seed_account(address, balance).unwrap();
    (temp_dir, ledger)
}

/// Helper: build and sign a simple (non-transfer) transaction.
fn build_signed_tx(keypair: &Keypair, nonce: u64, fee: u128, gas_limit: u64) -> Transaction {
    let address = Address::from_slice(&keypair.to_address()).unwrap();
    let mut tx = Transaction {
        nonce,
        chain_id: 1,
        sender: address,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    tx
}

/// Helper: build and sign a transfer transaction.
fn build_transfer_tx(
    keypair: &Keypair,
    recipient: Address,
    amount: u128,
    nonce: u64,
    fee: u128,
) -> Transaction {
    let address = Address::from_slice(&keypair.to_address()).unwrap();
    let payload = TransferPayload {
        recipient,
        amount,
        memo: None,
    };
    let mut tx = Transaction {
        nonce,
        chain_id: 1,
        sender: address,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
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
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    tx
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Valid transactions with sufficient balance always succeed.
    #[test]
    fn valid_tx_always_succeeds(fee in 1u128..1_000, gas_limit in 1u64..100_000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let balance = fee + 1_000_000;
        let (_dir, mut ledger) = setup_ledger(&address, balance);

        let tx = build_signed_tx(&keypair, 0, fee, gas_limit);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));

        let after = ledger.get_account(&address).unwrap().unwrap();
        prop_assert_eq!(after.balance, balance - fee);
        prop_assert_eq!(after.nonce, 1);
    }

    /// Transactions with fee > balance always fail.
    #[test]
    fn insufficient_balance_always_fails(
        balance in 0u128..1_000,
        extra in 1u128..10_000,
    ) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let fee = balance + extra;
        let (_dir, mut ledger) = setup_ledger(&address, balance);

        let tx = build_signed_tx(&keypair, 0, fee, 21_000);
        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());

        let after = ledger.get_account(&address).unwrap().unwrap();
        prop_assert_eq!(after.balance, balance);
    }

    /// Wrong nonce (non-zero when account is fresh) always causes rejection.
    #[test]
    fn wrong_nonce_rejected(bad_nonce in 1u64..1000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        // Account nonce is 0, any non-zero nonce should fail
        let tx = build_signed_tx(&keypair, bad_nonce, 100, 21_000);
        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }

    /// Invalid signature always fails.
    #[test]
    fn invalid_signature_always_rejected(fee in 1u128..1000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        let mut tx = build_signed_tx(&keypair, 0, fee, 21_000);
        tx.signature = Signature::from_bytes(vec![0xDE; 64]);

        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }

    /// Transfer amount + fee > balance always fails.
    #[test]
    fn transfer_exceeding_balance_fails(
        balance in 100u128..10_000,
        transfer_frac in 0.5f64..1.0,
        fee_frac in 0.5f64..1.0,
    ) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xAA; 20]).unwrap();

        let transfer_amount = ((balance as f64) * transfer_frac) as u128 + 1;
        let fee = ((balance as f64) * fee_frac) as u128 + 1;
        if transfer_amount + fee <= balance {
            return Ok(());
        }

        let (_dir, mut ledger) = setup_ledger(&address, balance);
        let tx = build_transfer_tx(&keypair, recipient, transfer_amount, 0, fee);
        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }

    /// Valid transfers correctly move funds between accounts.
    #[test]
    fn valid_transfer_conserves_value(
        transfer_amount in 1u128..10_000,
        fee in 1u128..1_000,
    ) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xBB; 20]).unwrap();
        let balance = transfer_amount + fee + 1_000;

        let (_dir, mut ledger) = setup_ledger(&address, balance);
        let tx = build_transfer_tx(&keypair, recipient, transfer_amount, 0, fee);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));

        let sender_after = ledger.get_account(&address).unwrap().unwrap();
        let recipient_after = ledger.get_account(&recipient).unwrap().unwrap();

        prop_assert_eq!(sender_after.balance, balance - transfer_amount - fee);
        prop_assert_eq!(recipient_after.balance, transfer_amount);
    }

    /// Sequential transactions must have sequential nonces.
    #[test]
    fn sequential_nonces_succeed(count in 1usize..10) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let fee_per_tx = 100u128;
        let balance = fee_per_tx * (count as u128) + 10_000;

        let (_dir, mut ledger) = setup_ledger(&address, balance);

        for i in 0..count {
            let tx = build_signed_tx(&keypair, i as u64, fee_per_tx, 21_000);
            let receipt = ledger.apply_transaction(&tx).unwrap();
            prop_assert!(matches!(receipt.status, TransactionStatus::Success));
        }

        let after = ledger.get_account(&address).unwrap().unwrap();
        prop_assert_eq!(after.nonce, count as u64);
        prop_assert_eq!(after.balance, balance - fee_per_tx * (count as u128));
    }

    /// Replaying the same nonce always fails on the second attempt.
    #[test]
    fn replay_attack_rejected(fee in 1u128..1_000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        let tx = build_signed_tx(&keypair, 0, fee, 21_000);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));

        // Replay same transaction — should fail on nonce
        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }

    /// Zero-amount transfers are rejected (amount must be > 0).
    #[test]
    fn zero_amount_transfer_rejected(fee in 1u128..1_000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xCC; 20]).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        let tx = build_transfer_tx(&keypair, recipient, 0, 0, fee);
        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }

    /// State root changes after every successful transaction.
    #[test]
    fn state_root_changes_on_success(fee in 1u128..1_000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        let root_before = ledger.state_root();
        let tx = build_signed_tx(&keypair, 0, fee, 21_000);
        ledger.apply_transaction(&tx).unwrap();
        let root_after = ledger.state_root();

        prop_assert_ne!(root_before, root_after);
    }

    /// A transaction signed by a different key than the sender is rejected.
    #[test]
    fn wrong_signer_rejected(fee in 1u128..1_000) {
        let real_keypair = Keypair::generate();
        let real_address = Address::from_slice(&real_keypair.to_address()).unwrap();
        let wrong_keypair = Keypair::generate();
        let (_dir, mut ledger) = setup_ledger(&real_address, 1_000_000);

        // Build tx for real_address but sign with wrong key
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: real_address,
            sender_pubkey: PublicKey::from_bytes(real_keypair.public_key()),
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(wrong_keypair.sign(hash.as_bytes()));

        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }

    /// Speculative execution produces the same state root as sequential apply.
    #[test]
    fn speculative_matches_sequential(
        count in 1usize..6,
        fee in 1u128..500,
    ) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let balance = fee * (count as u128) + 1_000_000;

        // Path A: speculative
        let dir_a = TempDir::new().unwrap();
        let storage_a = Storage::open(dir_a.path()).unwrap();
        let mut ledger_a = Ledger::new(storage_a).unwrap();
        ledger_a.seed_account(&address, balance).unwrap();

        let txs: Vec<_> = (0..count)
            .map(|n| build_signed_tx(&keypair, n as u64, fee, 21_000))
            .collect();
        let (_receipts, overlay) = ledger_a.apply_block_speculatively(&txs).unwrap();
        ledger_a.commit_overlay(overlay).unwrap();
        let root_a = ledger_a.state_root();

        // Path B: sequential
        let dir_b = TempDir::new().unwrap();
        let storage_b = Storage::open(dir_b.path()).unwrap();
        let mut ledger_b = Ledger::new(storage_b).unwrap();
        ledger_b.seed_account(&address, balance).unwrap();

        for tx in &txs {
            ledger_b.apply_transaction(tx).unwrap();
        }
        let root_b = ledger_b.state_root();

        prop_assert_eq!(root_a, root_b);
    }

    /// Self-transfers preserve total balance (only fee is deducted).
    #[test]
    fn self_transfer_only_deducts_fee(
        transfer_amount in 1u128..10_000,
        fee in 1u128..1_000,
    ) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let balance = transfer_amount + fee + 10_000;

        let (_dir, mut ledger) = setup_ledger(&address, balance);
        let tx = build_transfer_tx(&keypair, address, transfer_amount, 0, fee);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));

        let after = ledger.get_account(&address).unwrap().unwrap();
        // Self-transfer: amount goes back to sender, only fee is lost
        prop_assert_eq!(after.balance, balance - fee);
    }

    /// Cross-chain transactions are rejected in speculative execution.
    #[test]
    fn cross_chain_id_rejected_speculatively(
        wrong_chain_id in 2u64..1000,
        fee in 1u128..1_000,
    ) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        let tx = build_signed_tx(&keypair, 0, fee, 21_000);
        let (receipts, _overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[tx], Some(wrong_chain_id))
            .unwrap();

        prop_assert_eq!(receipts.len(), 1);
        match &receipts[0].status {
            TransactionStatus::Failed { reason } => {
                prop_assert!(reason.contains("wrong chain_id"));
            }
            other => prop_assert!(false, "expected Failed, got {:?}", other),
        }
    }

    /// Empty signature is always rejected.
    #[test]
    fn empty_signature_rejected(fee in 1u128..1_000) {
        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&address, 1_000_000);

        let mut tx = build_signed_tx(&keypair, 0, fee, 21_000);
        tx.signature = Signature::from_bytes(vec![]);

        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err());
    }
}
