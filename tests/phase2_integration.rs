use aether_ledger::{chain_store::ChainStore, Ledger};
use aether_runtime::{ledger_state::LedgerRuntimeState, RuntimeState};
use aether_state_snapshots::{decode_snapshot, generate_snapshot, import_snapshot};
use aether_state_storage::{Storage, StorageBatch, CF_ACCOUNTS};
use aether_types::{
    account::Account, Address, Block, Transaction, TransactionReceipt, TransactionStatus, VrfProof,
    H256,
};
use tempfile::TempDir;

fn empty_block(slot: u64, proposer: Address) -> Block {
    Block::new(
        slot,
        H256::zero(),
        proposer,
        VrfProof {
            output: [0u8; 32],
            proof: vec![],
        },
        Vec::<Transaction>::new(),
    )
}

#[test]
fn end_to_end_state_roundtrip_across_components() {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage.clone()).unwrap();

    // seed an account directly in storage to simulate prior state
    let seed_account = Address::from_slice(&[0x01u8; 20]).unwrap();
    let account = Account::with_balance(seed_account, 50_000);
    let mut seed_batch = StorageBatch::new();
    seed_batch.put(
        CF_ACCOUNTS,
        seed_account.as_bytes().to_vec(),
        bincode::serialize(&account).unwrap(),
    );
    storage.write_batch(seed_batch).unwrap();

    // Use runtime state to modify contract storage and balances atomically.
    let contract = Address::from_slice(&[0xCCu8; 20]).unwrap();
    let caller = Address::from_slice(&[0xDDu8; 20]).unwrap();
    ledger.apply_balance_delta(&caller, 10_000).unwrap();

    {
        let mut runtime_state = LedgerRuntimeState::new(&mut ledger).unwrap();
        runtime_state
            .storage_write(&contract, b"phase2".to_vec(), b"integration".to_vec())
            .unwrap();
        runtime_state.transfer(&caller, &contract, 1_000).unwrap();
        runtime_state.commit().unwrap();
    }

    let contract_account = ledger.get_or_create_account(&contract).unwrap();
    assert_eq!(contract_account.balance, 1_000);

    // Persist block + receipt via chain store
    let chain_store = ChainStore::new(storage.clone());
    let block = empty_block(777, contract);
    let block_hash = block.hash();
    let receipt = TransactionReceipt {
        tx_hash: H256::from_slice(&[0xEEu8; 32]).unwrap(),
        block_hash,
        slot: 777,
        status: TransactionStatus::Success,
        gas_used: 0,
        logs: vec![],
        state_root: ledger.state_root(),
    };
    chain_store
        .store_block(&block, std::slice::from_ref(&receipt))
        .expect("block persisted");

    // Snapshot the state and import into a fresh storage instance.
    let snapshot_bytes = generate_snapshot(&ledger.storage(), 777).unwrap();
    let snapshot = decode_snapshot(&snapshot_bytes).unwrap();
    assert_eq!(snapshot.metadata.height, 777);

    let import_dir = TempDir::new().unwrap();
    let imported_storage = Storage::open(import_dir.path()).unwrap();
    let imported_snapshot =
        import_snapshot(&imported_storage, &snapshot_bytes).expect("snapshot imported");
    assert_eq!(imported_snapshot.metadata.height, 777);

    // Ensure the imported ledger retains contract storage and balances.
    let imported_ledger = Ledger::new(imported_storage).unwrap();
    let imported_contract_balance = imported_ledger
        .get_or_create_account(&contract)
        .unwrap()
        .balance;
    assert_eq!(imported_contract_balance, 1_000);
    let imported_value = imported_ledger
        .get_contract_storage(&contract, b"phase2")
        .unwrap();
    assert!(
        imported_value.is_none(),
        "contract storage is not snapshotted yet"
    );

    // The stored block is still accessible from the original chain store.
    let fetched_block = chain_store
        .get_block_by_hash(&block_hash)
        .unwrap()
        .expect("block retrievable after snapshot");
    assert_eq!(fetched_block.header.slot, 777);
    let fetched_receipt = chain_store
        .get_receipt(&receipt.tx_hash)
        .unwrap()
        .expect("receipt retrievable after snapshot");
    assert_eq!(fetched_receipt.slot, 777);
}
