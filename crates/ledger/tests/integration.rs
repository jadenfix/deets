use aether_ledger::{chain_store::ChainStore, Ledger};
use aether_state_storage::Storage;
use aether_types::{Address, Block, Transaction, TransactionReceipt, TransactionStatus, H256};
use tempfile::TempDir;

fn empty_block(slot: u64, proposer: Address) -> Block {
    Block::new(
        slot,
        H256::zero(),
        proposer,
        aether_types::VrfProof {
            output: [0u8; 32],
            proof: vec![],
        },
        Vec::<Transaction>::new(),
    )
}

#[test]
fn contract_storage_and_chain_store_cooperate() {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage.clone()).unwrap();

    let contract = Address::from_slice(&[0x41u8; 20]).unwrap();
    ledger
        .set_contract_storage(&contract, b"key".to_vec(), b"value".to_vec())
        .unwrap();
    let account = ledger.update_account_storage_root(&contract).unwrap();
    assert_ne!(account.storage_root, H256::zero());

    let retrieved = ledger
        .get_contract_storage(&contract, b"key")
        .unwrap()
        .expect("storage persisted");
    assert_eq!(retrieved, b"value");

    // also ensure balance delta persists to storage
    ledger.apply_balance_delta(&contract, 2_000).unwrap();
    let stored = ledger.get_or_create_account(&contract).unwrap();
    assert_eq!(stored.balance, 2_000);

    // Persist a block via ChainStore using same underlying storage.
    let chain_store = ChainStore::new(storage);
    let block = empty_block(7, contract);
    let receipt = TransactionReceipt {
        tx_hash: H256::from_slice(&[0x55u8; 32]).unwrap(),
        block_hash: block.hash(),
        slot: 7,
        status: TransactionStatus::Success,
        gas_used: 0,
        logs: vec![],
        state_root: ledger.state_root(),
    };
    chain_store
        .store_block(&block, std::slice::from_ref(&receipt))
        .expect("block stored");

    let fetched = chain_store
        .get_block_by_hash(&block.hash())
        .unwrap()
        .expect("block retrievable");
    assert_eq!(fetched.header.slot, 7);

    let fetched_receipt = chain_store
        .get_receipt(&receipt.tx_hash)
        .unwrap()
        .expect("receipt retrievable");
    assert_eq!(fetched_receipt.slot, 7);
}
