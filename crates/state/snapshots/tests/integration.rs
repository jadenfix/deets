use aether_state_snapshots::{decode_snapshot, generate_snapshot, import_snapshot};
use aether_state_storage::{Storage, StorageBatch, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
use aether_types::{account::Account, Address, Utxo, UtxoId, H256};
use tempfile::TempDir;

#[test]
fn snapshot_roundtrip_preserves_state() {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();

    let address = Address::from_slice(&[0x11u8; 20]).unwrap();
    let account = Account::with_balance(address, 42_000);

    let utxo_id = UtxoId {
        tx_hash: H256::from_slice(&[0x22u8; 32]).unwrap(),
        output_index: 0,
    };
    let utxo = Utxo {
        amount: 10_000,
        owner: address,
        script_hash: None,
    };

    let mut batch = StorageBatch::new();
    batch.put(
        CF_ACCOUNTS,
        address.as_bytes().to_vec(),
        bincode::serialize(&account).unwrap(),
    );
    batch.put(
        CF_UTXOS,
        bincode::serialize(&utxo_id).unwrap(),
        bincode::serialize(&utxo).unwrap(),
    );
    storage.write_batch(batch).unwrap();
    storage
        .put(
            CF_METADATA,
            b"state_root",
            H256::from_slice(&[0x33u8; 32]).unwrap().as_bytes(),
        )
        .unwrap();

    let bytes = generate_snapshot(&storage, 64).expect("snapshot generation succeeds");
    let snapshot = decode_snapshot(&bytes).expect("snapshot decodes");
    assert_eq!(snapshot.metadata.height, 64);
    assert_eq!(snapshot.accounts.len(), 1);
    assert_eq!(snapshot.utxos.len(), 1);

    let dest_dir = TempDir::new().unwrap();
    let dest_storage = Storage::open(dest_dir.path()).unwrap();
    let imported = import_snapshot(&dest_storage, &bytes).expect("snapshot imports");

    assert_eq!(imported.metadata.height, snapshot.metadata.height);
    assert_eq!(imported.state_root, snapshot.state_root);
    assert_eq!(imported.accounts.len(), snapshot.accounts.len());
    assert_eq!(imported.utxos.len(), snapshot.utxos.len());

    let stored_account_bytes = dest_storage
        .get(CF_ACCOUNTS, address.as_bytes())
        .unwrap()
        .expect("account persisted");
    let stored_account: Account = bincode::deserialize(&stored_account_bytes).unwrap();
    assert_eq!(stored_account.balance, 42_000);

    let stored_utxo_bytes = dest_storage
        .get(CF_UTXOS, &bincode::serialize(&utxo_id).unwrap())
        .unwrap()
        .expect("utxo persisted");
    let stored_utxo: Utxo = bincode::deserialize(&stored_utxo_bytes).unwrap();
    assert_eq!(stored_utxo.amount, 10_000);
}
