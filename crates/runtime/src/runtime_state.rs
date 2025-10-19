use aether_types::{Address, H256};
use anyhow::Result;

/// Runtime State Interface
///
/// Provides an abstraction over blockchain state for contract execution.
/// This allows the runtime to be tested with mock state while using
/// real ledger state in production.
pub trait RuntimeState {
    /// Read from contract storage
    fn storage_read(&mut self, contract: &Address, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Write to contract storage
    fn storage_write(&mut self, contract: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()>;

    /// Get account balance
    fn get_balance(&self, address: &Address) -> Result<u128>;

    /// Transfer value between accounts
    fn transfer(&mut self, from: &Address, to: &Address, amount: u128) -> Result<()>;

    /// Emit a log event
    fn emit_log(&mut self, contract: &Address, topics: Vec<H256>, data: Vec<u8>) -> Result<()>;
}

/// Mock Runtime State for Testing
///
/// In-memory implementation for unit tests
pub struct MockRuntimeState {
    storage: std::collections::HashMap<(Address, Vec<u8>), Vec<u8>>,
    balances: std::collections::HashMap<Address, u128>,
    logs: Vec<(Address, Vec<H256>, Vec<u8>)>,
}

impl MockRuntimeState {
    pub fn new() -> Self {
        Self {
            storage: std::collections::HashMap::new(),
            balances: std::collections::HashMap::new(),
            logs: Vec::new(),
        }
    }

    pub fn with_balance(mut self, address: Address, balance: u128) -> Self {
        self.balances.insert(address, balance);
        self
    }

    pub fn get_logs(&self) -> &[(Address, Vec<H256>, Vec<u8>)] {
        &self.logs
    }
}

impl Default for MockRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeState for MockRuntimeState {
    fn storage_read(&mut self, contract: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.storage.get(&(*contract, key.to_vec())).cloned())
    }

    fn storage_write(&mut self, contract: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.storage.insert((*contract, key), value);
        Ok(())
    }

    fn get_balance(&self, address: &Address) -> Result<u128> {
        Ok(self.balances.get(address).copied().unwrap_or(0))
    }

    fn transfer(&mut self, from: &Address, to: &Address, amount: u128) -> Result<()> {
        let from_balance = self.balances.get(from).copied().unwrap_or(0);
        if from_balance < amount {
            anyhow::bail!("insufficient balance");
        }

        let to_balance = self.balances.get(to).copied().unwrap_or(0);

        self.balances.insert(*from, from_balance - amount);
        self.balances.insert(*to, to_balance + amount);

        Ok(())
    }

    fn emit_log(&mut self, contract: &Address, topics: Vec<H256>, data: Vec<u8>) -> Result<()> {
        self.logs.push((*contract, topics, data));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_storage() {
        let mut state = MockRuntimeState::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();

        // Write
        state
            .storage_write(&addr, b"key".to_vec(), b"value".to_vec())
            .unwrap();

        // Read
        let value = state.storage_read(&addr, b"key").unwrap();
        assert_eq!(value, Some(b"value".to_vec()));

        // Read non-existent
        let value = state.storage_read(&addr, b"other").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_mock_balance() {
        let state =
            MockRuntimeState::new().with_balance(Address::from_slice(&[1u8; 20]).unwrap(), 1000);

        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();

        assert_eq!(state.get_balance(&addr1).unwrap(), 1000);
        assert_eq!(state.get_balance(&addr2).unwrap(), 0);
    }

    #[test]
    fn test_mock_transfer() {
        let mut state =
            MockRuntimeState::new().with_balance(Address::from_slice(&[1u8; 20]).unwrap(), 1000);

        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();

        // Transfer
        state.transfer(&addr1, &addr2, 300).unwrap();

        assert_eq!(state.get_balance(&addr1).unwrap(), 700);
        assert_eq!(state.get_balance(&addr2).unwrap(), 300);
    }

    #[test]
    fn test_mock_transfer_insufficient() {
        let mut state =
            MockRuntimeState::new().with_balance(Address::from_slice(&[1u8; 20]).unwrap(), 100);

        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();

        // Should fail
        let result = state.transfer(&addr1, &addr2, 200);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_logs() {
        let mut state = MockRuntimeState::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();

        // Emit log
        state
            .emit_log(&addr, vec![H256::zero()], b"data".to_vec())
            .unwrap();

        let logs = state.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, addr);
        assert_eq!(logs[0].2, b"data");
    }
}
