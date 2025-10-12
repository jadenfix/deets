use aether_state_storage::{Storage, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
use aether_types::{account::Account, transaction::Utxo, transaction::UtxoId, Address, H256};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::compression::compress;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub height: u64,
    pub generated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub metadata: SnapshotMetadata,
    pub state_root: H256,
    pub accounts: Vec<(Address, Account)>,
    pub utxos: Vec<(UtxoId, Utxo)>,
}

pub fn generate_snapshot(storage: &Storage, height: u64) -> Result<Vec<u8>> {
    let accounts = load_accounts(storage)?;
    let utxos = load_utxos(storage)?;
    let state_root = load_state_root(storage)?;

    let metadata = SnapshotMetadata {
        height,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    let snapshot = StateSnapshot {
        metadata,
        state_root,
        accounts,
        utxos,
    };

    let encoded = bincode::serialize(&snapshot)?;
    Ok(compress(&encoded)?)
}

pub fn decode_snapshot(bytes: &[u8]) -> Result<StateSnapshot> {
    let raw = crate::compression::decompress(bytes)?;
    Ok(bincode::deserialize(&raw)?)
}

fn load_accounts(storage: &Storage) -> Result<Vec<(Address, Account)>> {
    let mut accounts = Vec::new();
    for (key, value) in storage.iterator(CF_ACCOUNTS)? {
        let address = Address::from_slice(&key).context("invalid address length")?;
        let account: Account = bincode::deserialize(&value)?;
        accounts.push((address, account));
    }
    Ok(accounts)
}

fn load_utxos(storage: &Storage) -> Result<Vec<(UtxoId, Utxo)>> {
    let mut utxos = Vec::new();
    for (key, value) in storage.iterator(CF_UTXOS)? {
        let id: UtxoId = bincode::deserialize(&key)?;
        let utxo: Utxo = bincode::deserialize(&value)?;
        utxos.push((id, utxo));
    }
    Ok(utxos)
}

fn load_state_root(storage: &Storage) -> Result<H256> {
    if let Some(bytes) = storage.get(CF_METADATA, b"state_root")? {
        return Ok(H256::from_slice(&bytes).context("invalid state root")?);
    }
    Ok(H256::zero())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_storage::Storage;
    use tempfile::TempDir;

    #[test]
    fn generate_roundtrip() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let bytes = generate_snapshot(&storage, 10).unwrap();
        let snapshot = decode_snapshot(&bytes).unwrap();
        assert_eq!(snapshot.metadata.height, 10);
    }
}
