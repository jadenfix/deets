use aether_state_storage::{Storage, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
use aether_types::{account::Account, Address, Utxo, UtxoId, H256};
use anyhow::{anyhow, bail, Result};
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
    compress(&encoded)
}

pub fn decode_snapshot(bytes: &[u8]) -> Result<StateSnapshot> {
    let raw = crate::compression::decompress(bytes)?;
    Ok(bincode::deserialize(&raw)?)
}

fn load_accounts(storage: &Storage) -> Result<Vec<(Address, Account)>> {
    let mut accounts = Vec::new();
    for (key, value) in storage.iterator(CF_ACCOUNTS)? {
        let address =
            Address::from_slice(&key).map_err(|e| anyhow!("invalid address length: {e}"))?;
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
        return H256::from_slice(&bytes).map_err(|e| anyhow!("invalid state root: {e}"));
    }
    bail!("state root not found in storage metadata — database may be uninitialized")
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
        // Seed a non-zero state root so generate_snapshot succeeds
        storage.put(CF_METADATA, b"state_root", &[1u8; 32]).unwrap();
        let bytes = generate_snapshot(&storage, 10).unwrap();
        let snapshot = decode_snapshot(&bytes).unwrap();
        assert_eq!(snapshot.metadata.height, 10);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_state_storage::{Storage, CF_METADATA};
    use proptest::prelude::*;
    use tempfile::TempDir;

    proptest! {
        /// Encode-then-decode preserves height for any u64 height.
        #[test]
        fn encode_decode_preserves_height(height in any::<u64>()) {
            let dir = TempDir::new().unwrap();
            let storage = Storage::open(dir.path()).unwrap();
            storage.put(CF_METADATA, b"state_root", &[1u8; 32]).unwrap();
            let bytes = generate_snapshot(&storage, height).unwrap();
            let snapshot = decode_snapshot(&bytes).unwrap();
            prop_assert_eq!(snapshot.metadata.height, height);
        }

        /// Encoding is deterministic for the same storage state.
        #[test]
        fn decode_encode_deterministic(height in 0u64..10000) {
            let dir = TempDir::new().unwrap();
            let storage = Storage::open(dir.path()).unwrap();
            storage.put(CF_METADATA, b"state_root", &[42u8; 32]).unwrap();
            let a = generate_snapshot(&storage, height).unwrap();
            let sa = decode_snapshot(&a).unwrap();
            let b = generate_snapshot(&storage, height).unwrap();
            let sb = decode_snapshot(&b).unwrap();
            // Heights and state roots must match; generated_at may differ by a second
            prop_assert_eq!(sa.metadata.height, sb.metadata.height);
            prop_assert_eq!(sa.state_root, sb.state_root);
            prop_assert_eq!(sa.accounts.len(), sb.accounts.len());
            prop_assert_eq!(sa.utxos.len(), sb.utxos.len());
        }

        /// Decode rejects truncated data.
        #[test]
        fn truncated_data_errors(height in 0u64..1000, cut in 1usize..64) {
            let dir = TempDir::new().unwrap();
            let storage = Storage::open(dir.path()).unwrap();
            storage.put(CF_METADATA, b"state_root", &[1u8; 32]).unwrap();
            let bytes = generate_snapshot(&storage, height).unwrap();
            if cut < bytes.len() {
                let truncated = &bytes[..bytes.len() - cut];
                prop_assert!(decode_snapshot(truncated).is_err());
            }
        }
    }
}
