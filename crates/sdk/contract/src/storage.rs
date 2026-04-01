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
        let mut store = s.lock().map_err(|e| ContractError::StorageError(format!("storage lock poisoned: {e}"))).expect("mock storage lock poisoned");
        store.clear();
    });
}

/// Read a u128 value from storage (convenience helper).
pub fn read_u128(key: &[u8]) -> ContractResult<u128> {
    match storage_read(key)? {
        Some(bytes) if bytes.len() == 16 => {
            let arr: [u8; 16] = bytes.try_into().map_err(|_| ContractError::StorageError("invalid u128 encoding: expected 16 bytes".into()))?;
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
