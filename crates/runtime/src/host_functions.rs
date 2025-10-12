use anyhow::Result;
use aether_types::{Address, H256};
use std::collections::HashMap;

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

pub struct HostFunctions {
    /// Contract storage (key -> value)
    storage: HashMap<Vec<u8>, Vec<u8>>,
    
    /// Account balances
    balances: HashMap<Address, u128>,
    
    /// Gas meter
    gas_used: u64,
    gas_limit: u64,
}

impl HostFunctions {
    pub fn new(gas_limit: u64) -> Self {
        HostFunctions {
            storage: HashMap::new(),
            balances: HashMap::new(),
            gas_used: 0,
            gas_limit,
        }
    }

    /// Read from contract storage
    /// Cost: 200 gas
    pub fn storage_read(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.charge_gas(200)?;
        Ok(self.storage.get(key).cloned())
    }

    /// Write to contract storage
    /// Cost: 5000 gas (expensive to incentivize minimal storage)
    pub fn storage_write(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.charge_gas(5000)?;
        
        // Charge extra for new keys
        if !self.storage.contains_key(&key) {
            self.charge_gas(20000)?; // New storage slot
        }
        
        self.storage.insert(key, value);
        Ok(())
    }

    /// Get account balance
    /// Cost: 100 gas
    pub fn get_balance(&mut self, address: &Address) -> Result<u128> {
        self.charge_gas(100)?;
        Ok(self.balances.get(address).copied().unwrap_or(0))
    }

    /// Transfer value to another account
    /// Cost: 9000 gas
    pub fn transfer(&mut self, from: &Address, to: &Address, amount: u128) -> Result<()> {
        self.charge_gas(9000)?;
        
        let from_balance = self.balances.get(from).copied().unwrap_or(0);
        if from_balance < amount {
            anyhow::bail!("insufficient balance");
        }
        
        let to_balance = self.balances.get(to).copied().unwrap_or(0);
        
        self.balances.insert(*from, from_balance - amount);
        self.balances.insert(*to, to_balance + amount);
        
        Ok(())
    }

    /// Compute SHA256 hash
    /// Cost: 60 gas + 12 gas per word
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
        
        // In production: store logs for receipts
        println!("LOG: topics={:?}, data_len={}", topics, data.len());
        
        Ok(())
    }

    /// Get current block number
    /// Cost: 2 gas
    pub fn block_number(&mut self) -> Result<u64> {
        self.charge_gas(2)?;
        Ok(1000) // Placeholder
    }

    /// Get current timestamp
    /// Cost: 2 gas
    pub fn timestamp(&mut self) -> Result<u64> {
        self.charge_gas(2)?;
        Ok(1234567890) // Placeholder
    }

    /// Get caller address
    /// Cost: 2 gas
    pub fn caller(&mut self) -> Result<Address> {
        self.charge_gas(2)?;
        Ok(Address::from_slice(&[1u8; 20]).unwrap()) // Placeholder
    }

    /// Get contract address
    /// Cost: 2 gas
    pub fn address(&mut self) -> Result<Address> {
        self.charge_gas(2)?;
        Ok(Address::from_slice(&[2u8; 20]).unwrap()) // Placeholder
    }

    fn charge_gas(&mut self, amount: u64) -> Result<()> {
        self.gas_used = self.gas_used.checked_add(amount)
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

    #[test]
    fn test_storage_operations() {
        let mut host = HostFunctions::new(100_000);
        
        // Write
        host.storage_write(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        
        // Read
        let value = host.storage_read(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
        
        // Read non-existent
        let value = host.storage_read(b"key2").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_balance_operations() {
        let mut host = HostFunctions::new(100_000);
        
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();
        
        // Initial balance
        host.balances.insert(addr1, 1000);
        
        assert_eq!(host.get_balance(&addr1).unwrap(), 1000);
        assert_eq!(host.get_balance(&addr2).unwrap(), 0);
    }

    #[test]
    fn test_transfer() {
        let mut host = HostFunctions::new(100_000);
        
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();
        
        host.balances.insert(addr1, 1000);
        
        // Transfer
        host.transfer(&addr1, &addr2, 300).unwrap();
        
        assert_eq!(host.get_balance(&addr1).unwrap(), 700);
        assert_eq!(host.get_balance(&addr2).unwrap(), 300);
    }

    #[test]
    fn test_transfer_insufficient_balance() {
        let mut host = HostFunctions::new(100_000);
        
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();
        
        host.balances.insert(addr1, 100);
        
        // Try to transfer more than balance
        let result = host.transfer(&addr1, &addr2, 200);
        assert!(result.is_err());
    }

    #[test]
    fn test_sha256() {
        let mut host = HostFunctions::new(100_000);
        
        let hash = host.sha256(b"hello world").unwrap();
        assert_eq!(hash.as_bytes().len(), 32);
    }

    #[test]
    fn test_gas_limits() {
        let mut host = HostFunctions::new(100); // Very low limit
        
        // First operation succeeds
        assert!(host.storage_read(b"key").is_ok());
        
        // Second operation should fail (out of gas)
        let result = host.storage_write(b"key".to_vec(), b"value".to_vec());
        assert!(result.is_err());
    }
}

