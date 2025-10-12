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
}
