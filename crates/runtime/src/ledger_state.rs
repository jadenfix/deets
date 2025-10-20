use std::collections::HashMap;

use aether_ledger::Ledger;
use aether_types::{Address, H256};
use anyhow::{anyhow, Result};

use crate::runtime_state::RuntimeState;

type ContractKey = (Address, Vec<u8>);
type StorageUpdate = (Vec<u8>, Vec<u8>);
type EventLog = (Address, Vec<H256>, Vec<u8>);

/// Ledger-backed Runtime State
///
/// Provides contract execution with access to persistent blockchain state.
/// Changes are cached and applied atomically after successful execution.
pub struct LedgerRuntimeState<'a> {
    ledger: &'a mut Ledger,
    base_state_root: H256,
    pending_storage: HashMap<ContractKey, Vec<u8>>,
    pending_balances: HashMap<Address, i128>,
    logs: Vec<EventLog>,
}

impl<'a> LedgerRuntimeState<'a> {
    pub fn new(ledger: &'a mut Ledger) -> Result<Self> {
        let base_state_root = ledger.state_root();
        Ok(Self {
            ledger,
            base_state_root,
            pending_storage: HashMap::new(),
            pending_balances: HashMap::new(),
            logs: Vec::new(),
        })
    }

    /// Commit all pending changes to the ledger.
    /// This applies storage writes, balance deltas, and returns collected logs.
    pub fn commit(mut self) -> Result<Vec<EventLog>> {
        self.apply_storage_writes()?;
        self.apply_balance_deltas()?;
        Ok(self.logs)
    }

    /// Get logs without consuming the state.
    pub fn logs(&self) -> &[(Address, Vec<H256>, Vec<u8>)] {
        &self.logs
    }

    /// Base state root captured when the runtime state was created.
    pub fn base_state_root(&self) -> H256 {
        self.base_state_root
    }

    fn apply_storage_writes(&mut self) -> Result<()> {
        if self.pending_storage.is_empty() {
            return Ok(());
        }

        let mut by_contract: HashMap<Address, Vec<StorageUpdate>> = HashMap::new();
        for ((contract, key), value) in self.pending_storage.drain() {
            by_contract.entry(contract).or_default().push((key, value));
        }

        for (contract, updates) in by_contract {
            for (key, value) in updates.iter() {
                self.ledger
                    .set_contract_storage(&contract, key.clone(), value.clone())?;
            }
            self.ledger.update_account_storage_root(&contract)?;
        }

        Ok(())
    }

    fn apply_balance_deltas(&mut self) -> Result<()> {
        for (address, delta) in self.pending_balances.drain() {
            self.ledger.apply_balance_delta(&address, delta)?;
        }
        Ok(())
    }
}

impl<'a> RuntimeState for LedgerRuntimeState<'a> {
    fn storage_read(&mut self, contract: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let key_vec = key.to_vec();
        if let Some(value) = self.pending_storage.get(&(*contract, key_vec.clone())) {
            return Ok(Some(value.clone()));
        }

        self.ledger.get_contract_storage(contract, key)
    }

    fn storage_write(&mut self, contract: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.pending_storage.insert((*contract, key), value);
        Ok(())
    }

    fn get_balance(&self, address: &Address) -> Result<u128> {
        let mut balance = self.ledger.get_or_create_account(address)?.balance;

        if let Some(delta) = self.pending_balances.get(address) {
            if *delta < 0 {
                let decrease = (-*delta) as u128;
                if balance < decrease {
                    return Err(anyhow!(
                        "pending balance delta would underflow account {:?}",
                        address
                    ));
                }
                balance -= decrease;
            } else {
                balance = balance
                    .checked_add(*delta as u128)
                    .ok_or_else(|| anyhow!("balance overflow for account {:?}", address))?;
            }
        }

        Ok(balance)
    }

    fn transfer(&mut self, from: &Address, to: &Address, amount: u128) -> Result<()> {
        let from_balance = self.get_balance(from)?;
        if from_balance < amount {
            return Err(anyhow!("insufficient balance for transfer"));
        }

        let from_delta = self.pending_balances.entry(*from).or_insert(0);
        *from_delta -= amount as i128;

        let to_delta = self.pending_balances.entry(*to).or_insert(0);
        *to_delta += amount as i128;

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
    use aether_state_storage::Storage;
    use aether_types::H256;
    use tempfile::TempDir;

    #[test]
    fn transfer_records_balance_deltas() {
        let _temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(_temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        let recipient = Address::from_slice(&[2u8; 20]).unwrap();

        ledger.apply_balance_delta(&sender, 1_000).unwrap();

        {
            let mut state = LedgerRuntimeState::new(&mut ledger).unwrap();
            state.transfer(&sender, &recipient, 400).unwrap();
            let logs = state.commit().unwrap();
            assert!(logs.is_empty());
        }

        let sender_account = ledger.get_or_create_account(&sender).unwrap();
        let recipient_account = ledger.get_or_create_account(&recipient).unwrap();
        assert_eq!(sender_account.balance, 600);
        assert_eq!(recipient_account.balance, 400);
    }

    #[test]
    fn storage_write_persists_on_commit() {
        let _temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(_temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        let contract = Address::from_slice(&[3u8; 20]).unwrap();

        {
            let mut state = LedgerRuntimeState::new(&mut ledger).unwrap();
            state
                .storage_write(&contract, b"key".to_vec(), b"value".to_vec())
                .unwrap();
            state.commit().unwrap();
        }

        let stored = ledger
            .get_contract_storage(&contract, b"key")
            .unwrap()
            .expect("storage value");
        assert_eq!(stored, b"value".to_vec());

        let account = ledger.get_or_create_account(&contract).unwrap();
        assert_ne!(account.storage_root, H256::zero());
    }

    #[test]
    fn logs_accumulate_during_execution() {
        let _temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(_temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        let contract = Address::from_slice(&[4u8; 20]).unwrap();
        let topic = H256::from_slice(&[9u8; 32]).unwrap();

        let mut state = LedgerRuntimeState::new(&mut ledger).unwrap();
        state
            .emit_log(&contract, vec![topic], b"data".to_vec())
            .unwrap();
        let logs = state.commit().unwrap();

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, contract);
        assert_eq!(logs[0].2, b"data".to_vec());
    }
}
