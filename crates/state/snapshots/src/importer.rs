use aether_state_storage::{Storage, StorageBatch, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
use anyhow::Result;

use crate::generator::{decode_snapshot, StateSnapshot};

pub fn import_snapshot(storage: &Storage, bytes: &[u8]) -> Result<StateSnapshot> {
    let snapshot = decode_snapshot(bytes)?;

    let mut batch = StorageBatch::new();
    for (address, account) in &snapshot.accounts {
        let key = address.as_bytes().to_vec();
        let value = bincode::serialize(account)?;
        batch.put(CF_ACCOUNTS, key, value);
    }

    for (id, utxo) in &snapshot.utxos {
        let key = bincode::serialize(id)?;
        let value = bincode::serialize(utxo)?;
        batch.put(CF_UTXOS, key, value);
    }

    storage.write_batch(batch)?;
    storage.put(CF_METADATA, b"state_root", snapshot.state_root.as_bytes())?;
    storage.put(
        CF_METADATA,
        b"snapshot_height",
        &snapshot.metadata.height.to_be_bytes(),
    )?;

    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_storage::Storage;
    use aether_types::{Account, Address, Utxo, UtxoId, H256};
    use tempfile::TempDir;

    #[test]
    fn imports_snapshot() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();

        let mut snapshot = StateSnapshot {
            metadata: crate::generator::SnapshotMetadata {
                height: 42,
                generated_at: 0,
            },
            state_root: H256::zero(),
            accounts: Vec::new(),
            utxos: Vec::new(),
        };

        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        snapshot.accounts.push((addr, Account::new(addr)));

        let utxo_id = UtxoId {
            tx_hash: H256::zero(),
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 10,
            owner: addr,
            script_hash: None,
        };
        snapshot.utxos.push((utxo_id, utxo));

        let bytes = crate::compression::compress(&bincode::serialize(&snapshot).unwrap()).unwrap();
        import_snapshot(&storage, &bytes).unwrap();
        assert!(storage
            .get(CF_METADATA, b"snapshot_height")
            .unwrap()
            .is_some());
    }

    #[test]
    #[ignore]
    fn phase4_snapshot_catch_up_benchmark() {
        use aether_state_storage::{StorageBatch, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
        use std::time::Instant;

        const ACCOUNT_COUNT: usize = 2_000;

        let source_dir = TempDir::new().unwrap();
        let source = Storage::open(source_dir.path()).unwrap();

        // Seed source storage with deterministic accounts and UTxOs
        let mut batch = StorageBatch::new();
        for i in 0..ACCOUNT_COUNT {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[..8].copy_from_slice(&(i as u64).to_be_bytes());
            let address = Address::from_slice(&addr_bytes).unwrap();
            let account = Account::with_balance(address, (i * 100) as u128);
            batch.put(
                CF_ACCOUNTS,
                address.as_bytes().to_vec(),
                bincode::serialize(&account).unwrap(),
            );

            let utxo_id = UtxoId {
                tx_hash: H256::zero(),
                output_index: i as u32,
            };
            let utxo = Utxo {
                amount: (i * 10) as u128,
                owner: address,
                script_hash: None,
            };
            batch.put(
                CF_UTXOS,
                bincode::serialize(&utxo_id).unwrap(),
                bincode::serialize(&utxo).unwrap(),
            );
        }
        source.write_batch(batch).unwrap();
        source
            .put(CF_METADATA, b"state_root", H256::zero().as_bytes())
            .unwrap();

        // Generate snapshot from populated storage
        let snapshot_bytes = crate::generator::generate_snapshot(&source, 100).unwrap();

        // Import snapshot into fresh storage and measure duration
        let target_dir = TempDir::new().unwrap();
        let target = Storage::open(target_dir.path()).unwrap();
        let start = Instant::now();
        let snapshot = import_snapshot(&target, &snapshot_bytes).unwrap();
        let elapsed = start.elapsed();

        // Basic correctness checks
        assert_eq!(snapshot.metadata.height, 100);
        assert_eq!(snapshot.accounts.len(), ACCOUNT_COUNT);
        assert!(
            elapsed.as_secs_f64() < 2.0,
            "snapshot import took {:?}",
            elapsed
        );

        // Ensure snapshot height metadata persisted
        let stored_height = target
            .get(CF_METADATA, b"snapshot_height")
            .unwrap()
            .map(|bytes| {
                let mut array = [0u8; 8];
                array.copy_from_slice(&bytes);
                u64::from_be_bytes(array)
            })
            .unwrap();
        assert_eq!(stored_height, 100);
    }
}
