//! Adversarial property-based tests for the Aether ledger.
//!
//! These tests probe security-critical invariants that an attacker would
//! target: balance conservation, identity binding, overflow boundaries,
//! speculative block isolation under combined overdraft, and state root
//! determinism.

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

fn setup_ledger(address: &Address, balance: u128) -> (TempDir, Ledger) {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();
    ledger.seed_account(address, balance).unwrap();
    (temp_dir, ledger)
}

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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Sender-pubkey identity attack: tx claims sender=victim but uses
    /// attacker's pubkey and signature. Must be rejected — if it isn't,
    /// any attacker can drain any account.
    #[test]
    fn prop_sender_pubkey_mismatch_rejected(
        amount in 1u128..10_000,
        fee in 1u128..1_000,
    ) {
        let victim_kp = Keypair::generate();
        let attacker_kp = Keypair::generate();
        let victim = Address::from_slice(&victim_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xDD; 20]).unwrap();

        let (_dir, mut ledger) = setup_ledger(&victim, 1_000_000);

        let payload = TransferPayload { recipient, amount, memo: None };
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: victim,
            sender_pubkey: PublicKey::from_bytes(attacker_kp.public_key()),
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
        tx.signature = Signature::from_bytes(attacker_kp.sign(hash.as_bytes()));

        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err(), "sender-pubkey mismatch must be rejected");

        let victim_acc = ledger.get_account(&victim).unwrap().unwrap();
        prop_assert_eq!(victim_acc.balance, 1_000_000, "victim balance must be unchanged");
    }

    /// Balance conservation: for a block of transfers between N senders,
    /// total system balance after execution equals total before minus total fees.
    #[test]
    fn prop_multi_sender_block_conserves_value(
        n_senders in 2usize..=5,
        amount in 10u128..1_000,
        fee in 1u128..100,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let initial_balance = amount + fee + 50_000;
        let mut keypairs = Vec::new();
        let mut addresses = Vec::new();
        for _ in 0..n_senders {
            let kp = Keypair::generate();
            let addr = Address::from_slice(&kp.to_address()).unwrap();
            ledger.seed_account(&addr, initial_balance).unwrap();
            keypairs.push(kp);
            addresses.push(addr);
        }

        let total_before = initial_balance * (n_senders as u128);

        // Each sender transfers to the next sender (circular)
        let txs: Vec<_> = (0..n_senders).map(|i| {
            let recipient = addresses[(i + 1) % n_senders];
            build_transfer_tx(&keypairs[i], recipient, amount, 0, fee)
        }).collect();

        let (receipts, overlay) = ledger
            .apply_block_speculatively_with_chain_id(&txs, Some(1))
            .unwrap();

        let successes: usize = receipts.iter()
            .filter(|r| matches!(r.status, TransactionStatus::Success))
            .count();

        ledger.commit_overlay(overlay).unwrap();

        let total_after: u128 = addresses.iter()
            .map(|a| ledger.get_account(a).unwrap().unwrap().balance)
            .sum();

        let total_fees = fee * (successes as u128);
        prop_assert_eq!(
            total_after, total_before - total_fees,
            "conservation violated: before={}, after={}, fees={}",
            total_before, total_after, total_fees
        );
    }

    /// Combined overdraft: two txs from the same sender in one block,
    /// each individually affordable but together exceeding balance.
    /// The second tx must fail without corrupting the first.
    #[test]
    fn prop_combined_overdraft_second_tx_fails(
        balance in 1000u128..10_000,
        split_frac in 0.6f64..0.9,
    ) {
        let kp = Keypair::generate();
        let sender = Address::from_slice(&kp.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xEE; 20]).unwrap();

        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        ledger.seed_account(&sender, balance).unwrap();

        let amount1 = ((balance as f64) * split_frac) as u128;
        let fee = 10u128;
        let amount2 = balance - amount1 + 1; // guarantees overdraft

        let tx1 = build_transfer_tx(&kp, recipient, amount1, 0, fee);
        let tx2 = build_transfer_tx(&kp, recipient, amount2, 1, fee);

        let (receipts, overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[tx1, tx2], Some(1))
            .unwrap();

        prop_assert_eq!(receipts.len(), 2);
        prop_assert!(
            matches!(receipts[0].status, TransactionStatus::Success),
            "first tx should succeed, got {:?}", receipts[0].status
        );
        prop_assert!(
            matches!(receipts[1].status, TransactionStatus::Failed { .. }),
            "second tx should fail (overdraft), got {:?}", receipts[1].status
        );

        ledger.commit_overlay(overlay).unwrap();
        let sender_acc = ledger.get_account(&sender).unwrap().unwrap();
        let recipient_acc = ledger.get_or_create_account(&recipient).unwrap();

        // Conservation: sender lost amount1 + fee, recipient gained amount1
        prop_assert_eq!(sender_acc.balance, balance - amount1 - fee);
        prop_assert_eq!(recipient_acc.balance, amount1);
    }

    /// Near-overflow: transfer amount close to u128::MAX must not panic
    /// or wrap around. The ledger must reject gracefully.
    #[test]
    fn prop_near_u128_max_no_panic(offset in 0u128..1000) {
        let kp = Keypair::generate();
        let sender = Address::from_slice(&kp.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xFF; 20]).unwrap();

        let amount = u128::MAX - offset;
        let (_dir, mut ledger) = setup_ledger(&sender, 1_000_000);

        let tx = build_transfer_tx(&kp, recipient, amount, 0, 1);
        let _ = ledger.apply_transaction(&tx); // must not panic
    }

    /// Fee-plus-amount overflow: fee + amount > u128::MAX must be caught
    /// by checked arithmetic, not wrap around.
    #[test]
    fn prop_fee_amount_overflow_rejected(
        fee_offset in 1u128..1000,
        amount_offset in 1u128..1000,
    ) {
        let kp = Keypair::generate();
        let sender = Address::from_slice(&kp.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xAA; 20]).unwrap();

        let amount = u128::MAX - amount_offset;
        let fee = fee_offset + amount_offset + 1; // guarantees overflow
        let (_dir, mut ledger) = setup_ledger(&sender, u128::MAX);

        let tx = build_transfer_tx(&kp, recipient, amount, 0, fee);
        let result = ledger.apply_transaction(&tx);
        prop_assert!(result.is_err(), "fee+amount overflow must be rejected");
    }

    /// Drain-to-exact-zero: fee exactly equals balance, nonce increments,
    /// and balance reaches exactly 0.
    #[test]
    fn prop_drain_to_zero_succeeds(balance in 1u128..100_000) {
        let kp = Keypair::generate();
        let sender = Address::from_slice(&kp.to_address()).unwrap();
        let (_dir, mut ledger) = setup_ledger(&sender, balance);

        let tx = build_signed_tx(&kp, 0, balance, 21_000);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));

        let acc = ledger.get_account(&sender).unwrap().unwrap();
        prop_assert_eq!(acc.balance, 0);
        prop_assert_eq!(acc.nonce, 1);

        // Further transactions must fail (zero balance)
        let tx2 = build_signed_tx(&kp, 1, 1, 21_000);
        let result = ledger.apply_transaction(&tx2);
        prop_assert!(result.is_err(), "zero-balance account must reject further txs");
    }

    /// State root determinism: applying the same transactions in the same
    /// order on two independent ledgers produces identical state roots.
    #[test]
    fn prop_state_root_deterministic(
        n_txs in 1usize..=8,
        fee in 1u128..100,
    ) {
        let kps: Vec<_> = (0..n_txs).map(|_| Keypair::generate()).collect();
        let recipient = Address::from_slice(&[0xBB; 20]).unwrap();

        let mut roots = Vec::new();
        for _ in 0..2 {
            let temp_dir = TempDir::new().unwrap();
            let storage = Storage::open(temp_dir.path()).unwrap();
            let mut ledger = Ledger::new(storage).unwrap();

            for kp in &kps {
                let addr = Address::from_slice(&kp.to_address()).unwrap();
                ledger.seed_account(&addr, 1_000_000).unwrap();
            }

            for kp in &kps {
                let tx = build_transfer_tx(kp, recipient, 100, 0, fee);
                let _ = ledger.apply_transaction(&tx);
            }
            roots.push(ledger.state_root());
        }

        prop_assert_eq!(roots[0], roots[1], "state roots must be deterministic");
    }

    /// Speculative block with invalid signature is rejected entirely.
    /// A valid block must never contain invalid-signature transactions.
    #[test]
    fn prop_speculative_block_rejects_bad_sig_entirely(fee in 1u128..1000) {
        let good_kp = Keypair::generate();
        let bad_kp = Keypair::generate();
        let good_addr = Address::from_slice(&good_kp.to_address()).unwrap();
        let bad_addr = Address::from_slice(&bad_kp.to_address()).unwrap();
        let recipient = Address::from_slice(&[0xCC; 20]).unwrap();

        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        ledger.seed_account(&good_addr, 1_000_000).unwrap();
        ledger.seed_account(&bad_addr, 1_000_000).unwrap();

        let good_tx = build_transfer_tx(&good_kp, recipient, 100, 0, fee);
        // Build a tx with tampered signature
        let mut bad_tx = build_transfer_tx(&bad_kp, recipient, 100, 0, fee);
        bad_tx.signature = Signature::from_bytes(vec![0xDE; 64]);

        let result = ledger.apply_block_speculatively_with_chain_id(
            &[good_tx, bad_tx], Some(1)
        );

        prop_assert!(result.is_err(), "block with bad signature must be rejected entirely");

        // Neither account's balance should have changed
        let good_acc = ledger.get_account(&good_addr).unwrap().unwrap();
        let bad_acc = ledger.get_account(&bad_addr).unwrap().unwrap();
        prop_assert_eq!(good_acc.balance, 1_000_000);
        prop_assert_eq!(bad_acc.balance, 1_000_000);
    }

    /// Transfer to nonexistent recipient creates the account with correct balance.
    #[test]
    fn prop_transfer_creates_recipient_account(
        amount in 1u128..10_000,
        fee in 1u128..1_000,
        recipient_seed in proptest::array::uniform20(any::<u8>()),
    ) {
        let kp = Keypair::generate();
        let sender = Address::from_slice(&kp.to_address()).unwrap();
        let recipient = Address::from_slice(&recipient_seed).unwrap();
        // Skip if recipient happens to equal sender
        prop_assume!(sender != recipient);

        let balance = amount + fee + 10_000;
        let (_dir, mut ledger) = setup_ledger(&sender, balance);

        // Recipient does not exist yet
        prop_assert!(ledger.get_account(&recipient).unwrap().is_none());

        let tx = build_transfer_tx(&kp, recipient, amount, 0, fee);
        let receipt = ledger.apply_transaction(&tx).unwrap();
        prop_assert!(matches!(receipt.status, TransactionStatus::Success));

        let rec_acc = ledger.get_account(&recipient).unwrap().unwrap();
        prop_assert_eq!(rec_acc.balance, amount);
        prop_assert_eq!(rec_acc.nonce, 0);
    }
}
