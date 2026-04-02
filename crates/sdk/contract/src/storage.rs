use crate::error::{ContractError, ContractResult};
use std::collections::HashMap;
use std::sync::Mutex;

// In WASM builds, these would be extern "C" host function imports.
// For native testing, we use a thread-local mock storage.
thread_local! {
    static MOCK_STORAGE: Mutex<HashMap<Vec<u8>, Vec<u8>>> = Mutex::new(HashMap::new());
}

/// Read a value from contract storage.
///
/// In WASM: calls the `env.storage_read` host function.
/// In tests: uses mock in-memory storage.
pub fn storage_read(key: &[u8]) -> ContractResult<Option<Vec<u8>>> {
    MOCK_STORAGE.with(|s| {
        let store = s
            .lock()
            .map_err(|e| ContractError::StorageError(e.to_string()))?;
        Ok(store.get(key).cloned())
    })
}

/// Write a value to contract storage.
///
/// In WASM: calls the `env.storage_write` host function.
/// In tests: uses mock in-memory storage.
pub fn storage_write(key: &[u8], value: &[u8]) -> ContractResult<()> {
    MOCK_STORAGE.with(|s| {
        let mut store = s
            .lock()
            .map_err(|e| ContractError::StorageError(e.to_string()))?;
        store.insert(key.to_vec(), value.to_vec());
        Ok(())
    })
}

/// Delete a value from contract storage.
pub fn storage_delete(key: &[u8]) -> ContractResult<()> {
    MOCK_STORAGE.with(|s| {
        let mut store = s
            .lock()
            .map_err(|e| ContractError::StorageError(e.to_string()))?;
        store.remove(key);
        Ok(())
    })
}

/// Clear all mock storage (for test isolation).
pub fn clear_mock_storage() {
    MOCK_STORAGE.with(|s| {
        let mut store = s
            .lock()
            .map_err(|e| ContractError::StorageError(format!("storage lock poisoned: {e}")))
            .expect("mock storage lock poisoned");
        store.clear();
    });
}

/// Read a u128 value from storage (convenience helper).
pub fn read_u128(key: &[u8]) -> ContractResult<u128> {
    match storage_read(key)? {
        Some(bytes) if bytes.len() == 16 => {
            let arr: [u8; 16] = bytes.try_into().map_err(|_| {
                ContractError::StorageError("invalid u128 encoding: expected 16 bytes".into())
            })?;
            Ok(u128::from_le_bytes(arr))
        }
        Some(_) => Err(ContractError::StorageError("invalid u128 encoding".into())),
        None => Ok(0),
    }
}

/// Write a u128 value to storage (convenience helper).
pub fn write_u128(key: &[u8], value: u128) -> ContractResult<()> {
    storage_write(key, &value.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_read_write() {
        clear_mock_storage();

        storage_write(b"key1", b"value1").unwrap();
        let val = storage_read(b"key1").unwrap();
        assert_eq!(val, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_storage_missing_key() {
        clear_mock_storage();

        let val = storage_read(b"nonexistent").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_storage_delete() {
        clear_mock_storage();

        storage_write(b"key", b"value").unwrap();
        storage_delete(b"key").unwrap();
        assert_eq!(storage_read(b"key").unwrap(), None);
    }

    #[test]
    fn test_u128_helpers() {
        clear_mock_storage();

        write_u128(b"balance", 1_000_000).unwrap();
        assert_eq!(read_u128(b"balance").unwrap(), 1_000_000);
    }

    #[test]
    fn test_u128_default_zero() {
        clear_mock_storage();

        assert_eq!(read_u128(b"unset").unwrap(), 0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any byte key/value pair roundtrips through storage.
        #[test]
        fn storage_write_read_roundtrip(
            key in prop::collection::vec(any::<u8>(), 1..64),
            value in prop::collection::vec(any::<u8>(), 0..256),
        ) {
            clear_mock_storage();
            storage_write(&key, &value).unwrap();
            let read = storage_read(&key).unwrap();
            prop_assert_eq!(read, Some(value));
        }

        /// Deleting a key makes it return None.
        #[test]
        fn storage_delete_removes(
            key in prop::collection::vec(any::<u8>(), 1..64),
            value in prop::collection::vec(any::<u8>(), 1..128),
        ) {
            clear_mock_storage();
            storage_write(&key, &value).unwrap();
            storage_delete(&key).unwrap();
            prop_assert_eq!(storage_read(&key).unwrap(), None);
        }

        /// Writing to the same key overwrites the previous value.
        #[test]
        fn storage_overwrite(
            key in prop::collection::vec(any::<u8>(), 1..64),
            v1 in prop::collection::vec(any::<u8>(), 0..128),
            v2 in prop::collection::vec(any::<u8>(), 0..128),
        ) {
            clear_mock_storage();
            storage_write(&key, &v1).unwrap();
            storage_write(&key, &v2).unwrap();
            prop_assert_eq!(storage_read(&key).unwrap(), Some(v2));
        }

        /// Any u128 value roundtrips through write_u128/read_u128.
        #[test]
        fn u128_roundtrip(key in prop::collection::vec(any::<u8>(), 1..32), value: u128) {
            clear_mock_storage();
            write_u128(&key, value).unwrap();
            let read = read_u128(&key).unwrap();
            prop_assert_eq!(read, value);
        }

        /// u128 zero is the default for missing keys.
        #[test]
        fn u128_missing_is_zero(key in prop::collection::vec(any::<u8>(), 1..32)) {
            clear_mock_storage();
            prop_assert_eq!(read_u128(&key).unwrap(), 0);
        }

        /// read_u128 rejects values that aren't exactly 16 bytes.
        #[test]
        fn u128_rejects_wrong_length(
            key in prop::collection::vec(any::<u8>(), 1..32),
            bad_len in (1usize..32).prop_filter("not 16", |l| *l != 16),
        ) {
            clear_mock_storage();
            let bad_value = vec![0xABu8; bad_len];
            storage_write(&key, &bad_value).unwrap();
            prop_assert!(read_u128(&key).is_err());
        }

        /// Multiple distinct keys are isolated from each other.
        #[test]
        fn key_isolation(
            entries in prop::collection::vec(
                (prop::collection::vec(any::<u8>(), 1..16), prop::collection::vec(any::<u8>(), 1..32)),
                1..10,
            )
        ) {
            clear_mock_storage();
            // Deduplicate keys — last value wins
            let mut map = std::collections::HashMap::new();
            for (k, v) in &entries {
                storage_write(k, v).unwrap();
                map.insert(k.clone(), v.clone());
            }
            for (k, expected) in &map {
                let actual = storage_read(k).unwrap();
                prop_assert_eq!(actual.as_ref(), Some(expected));
            }
        }

        /// Deleting one key doesn't affect another.
        #[test]
        fn delete_isolation(
            k1 in prop::collection::vec(any::<u8>(), 1..16),
            k2 in prop::collection::vec(any::<u8>(), 1..16),
            v1 in prop::collection::vec(any::<u8>(), 1..32),
            v2 in prop::collection::vec(any::<u8>(), 1..32),
        ) {
            prop_assume!(k1 != k2);
            clear_mock_storage();
            storage_write(&k1, &v1).unwrap();
            storage_write(&k2, &v2).unwrap();
            storage_delete(&k1).unwrap();
            prop_assert_eq!(storage_read(&k1).unwrap(), None);
            prop_assert_eq!(storage_read(&k2).unwrap(), Some(v2));
        }

        /// Deleting a nonexistent key is a no-op.
        #[test]
        fn delete_nonexistent_is_noop(key in prop::collection::vec(any::<u8>(), 1..32)) {
            clear_mock_storage();
            prop_assert!(storage_delete(&key).is_ok());
            prop_assert_eq!(storage_read(&key).unwrap(), None);
        }

        /// Empty value is a valid stored value (distinct from missing key).
        #[test]
        fn empty_value_is_some(key in prop::collection::vec(any::<u8>(), 1..32)) {
            clear_mock_storage();
            storage_write(&key, &[]).unwrap();
            prop_assert_eq!(storage_read(&key).unwrap(), Some(vec![]));
        }
    }
}
