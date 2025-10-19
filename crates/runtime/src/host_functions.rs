use aether_types::{Address, H256};
use anyhow::Result;

use crate::runtime_state::RuntimeState;

/// Host Functions for WASM Contracts
///
/// These functions are imported into the WASM environment and allow
/// contracts to interact with the blockchain state.
///
/// Security:
/// - All functions charge gas
/// - Memory access is bounds-checked
/// - State changes are atomic
/// - No access to host filesystem/network
///
pub struct HostFunctions<'a> {
    /// Runtime state (ledger-backed or mock)
    state: &'a mut dyn RuntimeState,

    /// Gas meter
    gas_used: u64,
    gas_limit: u64,

    /// Execution context (block info)
    block_number: u64,
    timestamp: u64,
    caller: Address,
    contract_address: Address,
}

impl<'a> HostFunctions<'a> {
    pub fn new(
        state: &'a mut dyn RuntimeState,
        gas_limit: u64,
        block_number: u64,
        timestamp: u64,
        caller: Address,
        contract_address: Address,
    ) -> Self {
        HostFunctions {
            state,
            gas_used: 0,
            gas_limit,
            block_number,
            timestamp,
            caller,
            contract_address,
        }
    }

    /// Read from contract storage
    /// Cost: 200 gas
    pub fn storage_read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.charge_gas(200)?;
        self.state.storage_read(&self.contract_address, key)
    }

    /// Write to contract storage
    /// Cost: 5000 gas (expensive to incentivize minimal storage)
    pub fn storage_write(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.charge_gas(5000)?;

        // Check if key exists (for extra charge on new slots)
        let exists = self
            .state
            .storage_read(&self.contract_address, &key)?
            .is_some();
        if !exists {
            self.charge_gas(20000)?; // New storage slot
        }

        self.state.storage_write(&self.contract_address, key, value)
    }

    /// Get account balance
    /// Cost: 100 gas
    pub fn get_balance(&mut self, address: &Address) -> Result<u128> {
        self.charge_gas(100)?;
        self.state.get_balance(address)
    }

    /// Transfer value to another account
    /// Cost: 9000 gas
    pub fn transfer(&mut self, from: &Address, to: &Address, amount: u128) -> Result<()> {
        self.charge_gas(9000)?;
        self.state.transfer(from, to, amount)
    }

    /// Compute SHA256 hash
    /// Cost: 60 gas + 12 gas per word
    #[allow(clippy::manual_div_ceil)]
    pub fn sha256(&mut self, data: &[u8]) -> Result<H256> {
        let words = (data.len() + 31) / 32;
        self.charge_gas(60 + 12 * words as u64)?;

        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(data);
        Ok(H256::from_slice(&hash).unwrap())
    }

    /// Emit a log event
    /// Cost: 375 gas + 8 gas per byte
    pub fn emit_log(&mut self, topics: Vec<H256>, data: Vec<u8>) -> Result<()> {
        self.charge_gas(375 + 8 * data.len() as u64)?;
        self.state.emit_log(&self.contract_address, topics, data)
    }

    /// Get current block number
    /// Cost: 2 gas
    pub fn block_number(&mut self) -> Result<u64> {
        self.charge_gas(2)?;
        Ok(self.block_number)
    }

    /// Get current timestamp
    /// Cost: 2 gas
    pub fn timestamp(&mut self) -> Result<u64> {
        self.charge_gas(2)?;
        Ok(self.timestamp)
    }

    /// Get caller address
    /// Cost: 2 gas
    pub fn caller(&mut self) -> Result<Address> {
        self.charge_gas(2)?;
        Ok(self.caller)
    }

    /// Get contract address
    /// Cost: 2 gas
    pub fn address(&mut self) -> Result<Address> {
        self.charge_gas(2)?;
        Ok(self.contract_address)
    }

    fn charge_gas(&mut self, amount: u64) -> Result<()> {
        self.gas_used = self
            .gas_used
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("gas overflow"))?;

        if self.gas_used > self.gas_limit {
            anyhow::bail!("out of gas");
        }

        Ok(())
    }

    pub fn gas_used(&self) -> u64 {
        self.gas_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_state::MockRuntimeState;

    #[test]
    fn test_storage_operations() {
        let mut state = MockRuntimeState::new();
        let addr = Address::from_slice(&[1u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            addr,
        );

        // Write
        host.storage_write(b"key1".to_vec(), b"value1".to_vec())
            .unwrap();

        // Read
        let value = host.storage_read(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Read non-existent
        let value = host.storage_read(b"key2").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_balance_operations() {
        let mut state =
            MockRuntimeState::new().with_balance(Address::from_slice(&[1u8; 20]).unwrap(), 1000);

        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            Address::from_slice(&[0u8; 20]).unwrap(),
        );

        assert_eq!(host.get_balance(&addr1).unwrap(), 1000);
        assert_eq!(host.get_balance(&addr2).unwrap(), 0);
    }

    #[test]
    fn test_transfer() {
        let mut state =
            MockRuntimeState::new().with_balance(Address::from_slice(&[1u8; 20]).unwrap(), 1000);

        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            Address::from_slice(&[0u8; 20]).unwrap(),
        );

        // Transfer
        host.transfer(&addr1, &addr2, 300).unwrap();

        assert_eq!(host.get_balance(&addr1).unwrap(), 700);
        assert_eq!(host.get_balance(&addr2).unwrap(), 300);
    }

    #[test]
    fn test_transfer_insufficient_balance() {
        let mut state =
            MockRuntimeState::new().with_balance(Address::from_slice(&[1u8; 20]).unwrap(), 100);

        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            Address::from_slice(&[0u8; 20]).unwrap(),
        );

        // Try to transfer more than balance
        let result = host.transfer(&addr1, &addr2, 200);
        assert!(result.is_err());
    }

    #[test]
    fn test_sha256() {
        let mut state = MockRuntimeState::new();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            Address::from_slice(&[0u8; 20]).unwrap(),
        );

        let hash = host.sha256(b"hello world").unwrap();
        assert_eq!(hash.as_bytes().len(), 32);
    }

    #[test]
    fn test_gas_limits() {
        let mut state = MockRuntimeState::new();

        let mut host = HostFunctions::new(
            &mut state,
            5_000, // Low limit
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            Address::from_slice(&[0u8; 20]).unwrap(),
        );

        // First operation succeeds
        assert!(host.storage_read(b"key").is_ok());

        // Second operation should fail (out of gas)
        let result = host.storage_write(b"key".to_vec(), b"value".to_vec());
        assert!(result.is_err());
    }

    #[test]
    fn test_context_functions() {
        let mut state = MockRuntimeState::new();

        let caller_addr = Address::from_slice(&[1u8; 20]).unwrap();
        let contract_addr = Address::from_slice(&[2u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            42,         // block_number
            1234567890, // timestamp
            caller_addr,
            contract_addr,
        );

        // Verify context values
        assert_eq!(host.block_number().unwrap(), 42);
        assert_eq!(host.timestamp().unwrap(), 1234567890);
        assert_eq!(host.caller().unwrap(), caller_addr);
        assert_eq!(host.address().unwrap(), contract_addr);
    }

    #[test]
    fn test_context_gas_charging() {
        let mut state = MockRuntimeState::new();

        let caller_addr = Address::from_slice(&[1u8; 20]).unwrap();
        let contract_addr = Address::from_slice(&[2u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100, // Low gas limit
            42,
            1234567890,
            caller_addr,
            contract_addr,
        );

        // Each context call costs 2 gas
        assert!(host.block_number().is_ok()); // 2 gas
        assert_eq!(host.gas_used(), 2);

        assert!(host.timestamp().is_ok()); // 2 gas
        assert_eq!(host.gas_used(), 4);

        assert!(host.caller().is_ok()); // 2 gas
        assert_eq!(host.gas_used(), 6);

        assert!(host.address().is_ok()); // 2 gas
        assert_eq!(host.gas_used(), 8);
    }

    #[test]
    fn test_emit_log() {
        let mut state = MockRuntimeState::new();
        let contract_addr = Address::from_slice(&[1u8; 20]).unwrap();

        let mut host = HostFunctions::new(
            &mut state,
            100_000,
            0,
            0,
            Address::from_slice(&[0u8; 20]).unwrap(),
            contract_addr,
        );

        // Emit log
        host.emit_log(vec![H256::zero()], b"test data".to_vec())
            .unwrap();

        // Verify log was recorded
        let logs = state.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, contract_addr);
        assert_eq!(logs[0].2, b"test data");
    }
}
