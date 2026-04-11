//! Integration tests for ledger speculative execution, overlay commit,
//! UTxO flows, credit_account, and fee burn accounting.
//!
//! These cover the critical gaps identified by QA: apply_block_speculatively,
//! commit_overlay, UTxO create/spend/double-spend, credit_account, burned fees,
//! chain_id mismatch, and speculative-vs-sequential equivalence.

use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_state_storage::Storage;
use aether_types::{
    Address, PublicKey, Signature, Transaction, TransactionStatus, TransferPayload, UtxoId, H256,
    TRANSFER_PROGRAM_ID,
};
use std::collections::HashSet;
use tempfile::TempDir;

/// Create a fresh ledger with a funded account.
fn setup(address: &Address, balance: u128) -> (TempDir, Ledger) {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();
    ledger.seed_account(address, balance).unwrap();
    (temp_dir, ledger)
}

/// Build and sign a simple fee-only transaction.
fn simple_tx(keypair: &Keypair, nonce: u64, fee: u128) -> Transaction {
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
        gas_limit: 21_000,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    tx
}

/// Build and sign a transfer transaction.
fn transfer_tx(
    keypair: &Keypair,
    nonce: u64,
    fee: u128,
    recipient: Address,
    amount: u128,
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

// ─── apply_block_speculatively ───────────────────────────────────────

#[test]
fn speculative_empty_block() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 1_000);

    let root_before = ledger.state_root();
    let (receipts, overlay) = ledger.apply_block_speculatively(&[]).unwrap();
    assert!(receipts.is_empty());
    assert_eq!(
        overlay.state_root, root_before,
        "empty block must not change state root"
    );
}

#[test]
fn speculative_single_tx() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 10_000);

    let tx = simple_tx(&keypair, 0, 100);
    let (receipts, overlay) = ledger.apply_block_speculatively(&[tx]).unwrap();

    assert_eq!(receipts.len(), 1);
    assert!(matches!(receipts[0].status, TransactionStatus::Success));
    assert_ne!(
        overlay.state_root,
        ledger.state_root(),
        "speculative root should differ from committed"
    );
}

#[test]
fn speculative_multiple_txs_sequential_nonces() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 100_000);

    let txs: Vec<_> = (0..5).map(|n| simple_tx(&keypair, n, 100)).collect();
    let (receipts, overlay) = ledger.apply_block_speculatively(&txs).unwrap();

    assert_eq!(receipts.len(), 5);
    for r in &receipts {
        assert!(
            matches!(r.status, TransactionStatus::Success),
            "tx failed: {:?}",
            r.status
        );
    }

    // Commit and verify account state
    let spec_root = overlay.state_root;
    ledger.commit_overlay(overlay).unwrap();

    let account = ledger.get_account(&addr).unwrap().unwrap();
    assert_eq!(account.nonce, 5);
    assert_eq!(account.balance, 100_000 - 500); // 5 * 100 fee
    assert_eq!(ledger.state_root(), spec_root);
}

// ─── commit_overlay persistence ──────────────────────────────────────

#[test]
fn commit_overlay_persists_to_storage() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let recipient_addr = Address::from_slice(&[7u8; 20]).unwrap();
    let (_dir, mut ledger) = setup(&addr, 50_000);

    let tx = transfer_tx(&keypair, 0, 200, recipient_addr, 5_000);
    let (receipts, overlay) = ledger.apply_block_speculatively(&[tx]).unwrap();
    assert!(matches!(receipts[0].status, TransactionStatus::Success));

    ledger.commit_overlay(overlay).unwrap();

    // Verify both accounts in storage
    let sender = ledger.get_account(&addr).unwrap().unwrap();
    assert_eq!(sender.balance, 50_000 - 200 - 5_000);
    assert_eq!(sender.nonce, 1);

    let recipient = ledger.get_account(&recipient_addr).unwrap().unwrap();
    assert_eq!(recipient.balance, 5_000);
}

// ─── speculative vs sequential equivalence ───────────────────────────

#[test]
fn speculative_matches_sequential_state_root() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();

    // Path A: speculative block execution + commit
    let dir_a = TempDir::new().unwrap();
    let storage_a = Storage::open(dir_a.path()).unwrap();
    let mut ledger_a = Ledger::new(storage_a).unwrap();
    ledger_a.seed_account(&addr, 100_000).unwrap();

    let txs: Vec<_> = (0..3).map(|n| simple_tx(&keypair, n, 100)).collect();
    let (_receipts, overlay) = ledger_a.apply_block_speculatively(&txs).unwrap();
    ledger_a.commit_overlay(overlay).unwrap();
    let root_a = ledger_a.state_root();

    // Path B: sequential apply_transaction
    let dir_b = TempDir::new().unwrap();
    let storage_b = Storage::open(dir_b.path()).unwrap();
    let mut ledger_b = Ledger::new(storage_b).unwrap();
    ledger_b.seed_account(&addr, 100_000).unwrap();

    for tx in &txs {
        ledger_b.apply_transaction(tx).unwrap();
    }
    let root_b = ledger_b.state_root();

    assert_eq!(
        root_a, root_b,
        "speculative+commit must produce same state root as sequential"
    );
}

// ─── chain_id mismatch ───────────────────────────────────────────────

#[test]
fn chain_id_mismatch_produces_failed_receipt() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 10_000);

    let tx = simple_tx(&keypair, 0, 100); // chain_id = 1
    let root_before = ledger.state_root();

    let (receipts, overlay) = ledger
        .apply_block_speculatively_with_chain_id(&[tx], Some(42))
        .unwrap();

    assert_eq!(receipts.len(), 1);
    match &receipts[0].status {
        TransactionStatus::Failed { reason } => {
            assert!(
                reason.contains("wrong chain_id"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Failed, got {:?}", other),
    }
    // State root unchanged for failed tx
    assert_eq!(overlay.state_root, root_before);
}

// ─── credit_account ─────────────────────────────────────────────────

#[test]
fn credit_account_increases_balance_and_updates_root() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 1_000);

    let root_before = ledger.state_root();
    ledger.credit_account(&addr, 500).unwrap();

    let account = ledger.get_account(&addr).unwrap().unwrap();
    assert_eq!(account.balance, 1_500);
    assert_ne!(ledger.state_root(), root_before);
}

#[test]
fn credit_account_creates_new_account() {
    let (_dir, mut ledger) = {
        let temp = TempDir::new().unwrap();
        let storage = Storage::open(temp.path()).unwrap();
        let ledger = Ledger::new(storage).unwrap();
        (temp, ledger)
    };

    let addr = Address::from_slice(&[0xAA; 20]).unwrap();
    ledger.credit_account(&addr, 999).unwrap();

    let account = ledger.get_account(&addr).unwrap().unwrap();
    assert_eq!(account.balance, 999);
}

#[test]
fn credit_account_overflow_rejected() {
    let addr = Address::from_slice(&[0xBB; 20]).unwrap();
    let (_dir, mut ledger) = setup(&addr, u128::MAX);

    let result = ledger.credit_account(&addr, 1);
    assert!(result.is_err(), "should reject overflow");
}

// ─── record_burned_fees / total_burned ──────────────────────────────

#[test]
fn burned_fees_round_trip() {
    let temp = TempDir::new().unwrap();
    let storage = Storage::open(temp.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();

    assert_eq!(ledger.total_burned(), 0);

    ledger.record_burned_fees(1_000).unwrap();
    assert_eq!(ledger.total_burned(), 1_000);

    ledger.record_burned_fees(500).unwrap();
    assert_eq!(ledger.total_burned(), 1_500);
}

#[test]
fn burned_fees_zero_is_noop() {
    let temp = TempDir::new().unwrap();
    let storage = Storage::open(temp.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();

    ledger.record_burned_fees(100).unwrap();
    ledger.record_burned_fees(0).unwrap();
    assert_eq!(ledger.total_burned(), 100);
}

#[test]
fn burned_fees_overflow_rejected() {
    let temp = TempDir::new().unwrap();
    let storage = Storage::open(temp.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();

    ledger.record_burned_fees(u128::MAX).unwrap();
    let result = ledger.record_burned_fees(1);
    assert!(
        result.is_err(),
        "recording burned fees past u128::MAX must error, not silently saturate"
    );
    assert_eq!(ledger.total_burned(), u128::MAX);
}

// ─── UTxO flow ──────────────────────────────────────────────────────

/// Seed a UTxO directly into storage and return its ID.
fn seed_utxo(ledger: &mut Ledger, owner: Address, amount: u128) -> UtxoId {
    use aether_state_storage::{StorageBatch, CF_UTXOS};
    use aether_types::Utxo;

    // Use a deterministic fake tx hash based on owner + amount
    let mut hash_input = owner.as_bytes().to_vec();
    hash_input.extend(&amount.to_le_bytes());
    let hash = {
        use sha2::{Digest, Sha256};
        H256::from_slice(&Sha256::digest(&hash_input)).unwrap()
    };

    let utxo_id = UtxoId {
        tx_hash: hash,
        output_index: 0,
    };
    let utxo = Utxo {
        amount,
        owner,
        script_hash: None,
    };
    let key = bincode::serialize(&utxo_id).unwrap();
    let value = bincode::serialize(&utxo).unwrap();
    let mut batch = StorageBatch::new();
    batch.put(CF_UTXOS, key, value);
    ledger.storage().write_batch(batch).unwrap();
    utxo_id
}

#[test]
fn utxo_create_and_spend() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 100_000);

    // Seed a UTxO owned by ourselves
    let utxo_id = seed_utxo(&mut ledger, addr, 500);

    // Verify UTxO exists
    let utxo = ledger.get_utxo(&utxo_id).unwrap();
    assert!(utxo.is_some());
    assert_eq!(utxo.unwrap().amount, 500);

    // Spend it (inputs consume the UTxO, no outputs so total_input >= total_output)
    let mut tx = Transaction {
        nonce: 0,
        chain_id: 1,
        sender: addr,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
        inputs: vec![utxo_id.clone()],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee: 50,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

    let receipt = ledger.apply_transaction(&tx).unwrap();
    assert!(matches!(receipt.status, TransactionStatus::Success));

    // UTxO should be consumed
    let utxo_after = ledger.get_utxo(&utxo_id).unwrap();
    assert!(utxo_after.is_none(), "spent UTxO should be deleted");
}

#[test]
fn utxo_double_spend_rejected() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 100_000);

    let utxo_id = seed_utxo(&mut ledger, addr, 500);

    // Spend it once
    let mut tx1 = Transaction {
        nonce: 0,
        chain_id: 1,
        sender: addr,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
        inputs: vec![utxo_id.clone()],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee: 50,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx1.hash();
    tx1.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    ledger.apply_transaction(&tx1).unwrap();

    // Try to spend it again
    let mut tx2 = Transaction {
        nonce: 1,
        chain_id: 1,
        sender: addr,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
        inputs: vec![utxo_id.clone()],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee: 50,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx2.hash();
    tx2.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

    let result = ledger.apply_transaction(&tx2);
    assert!(result.is_err(), "double-spend should be rejected");
}

#[test]
fn utxo_double_spend_within_block_rejected() {
    let keypair = Keypair::generate();
    let addr = Address::from_slice(&keypair.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr, 100_000);

    let utxo_id = seed_utxo(&mut ledger, addr, 500);

    // Two txs in same block both consuming the same UTxO
    let build_spend = |nonce: u64| -> Transaction {
        let mut tx = Transaction {
            nonce,
            chain_id: 1,
            sender: addr,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id.clone()],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 50,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
        tx
    };

    let tx1 = build_spend(0);
    let tx2 = build_spend(1);

    let (receipts, _overlay) = ledger.apply_block_speculatively(&[tx1, tx2]).unwrap();
    assert_eq!(receipts.len(), 2);
    assert!(matches!(receipts[0].status, TransactionStatus::Success));
    match &receipts[1].status {
        TransactionStatus::Failed { reason } => {
            assert!(
                reason.contains("already spent") || reason.contains("not found"),
                "unexpected failure reason: {reason}"
            );
        }
        other => panic!("expected double-spend to fail, got {:?}", other),
    }
}

#[test]
fn utxo_ownership_mismatch_rejected() {
    let keypair_a = Keypair::generate();
    let addr_a = Address::from_slice(&keypair_a.to_address()).unwrap();
    let keypair_b = Keypair::generate();
    let addr_b = Address::from_slice(&keypair_b.to_address()).unwrap();
    let (_dir, mut ledger) = setup(&addr_a, 100_000);
    ledger.seed_account(&addr_b, 100_000).unwrap();

    // Create UTxO owned by A
    let utxo_id = seed_utxo(&mut ledger, addr_a, 500);

    // B tries to spend A's UTxO
    let mut tx = Transaction {
        nonce: 0,
        chain_id: 1,
        sender: addr_b,
        sender_pubkey: PublicKey::from_bytes(keypair_b.public_key()),
        inputs: vec![utxo_id],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee: 50,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair_b.sign(hash.as_bytes()));

    let result = ledger.apply_transaction(&tx);
    assert!(result.is_err(), "spending someone else's UTxO should fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not sender"),
        "unexpected error: {err_msg}"
    );
}
