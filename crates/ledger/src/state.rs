use aether-types::{H256, Address, Account, Utxo, UtxoId, Transaction, TransactionReceipt, TransactionStatus};
use aether-state-storage::{Storage, StorageBatch, CF_ACCOUNTS, CF_UTXOS, CF_METADATA};
use aether-state-merkle::SparseMerkleTree;
use anyhow::{Result, Context, bail};
use std::collections::HashMap;

pub struct Ledger {
    storage: Storage,
    merkle_tree: SparseMerkleTree,
}

impl Ledger {
    pub fn new(storage: Storage) -> Result<Self> {
        let mut ledger = Ledger {
            storage,
            merkle_tree: SparseMerkleTree::new(),
        };
        
        ledger.load_state_root()?;
        Ok(ledger)
    }

    fn load_state_root(&mut self) -> Result<()> {
        if let Some(root_bytes) = self.storage.get(CF_METADATA, b"state_root")? {
            // In production, would reconstruct tree from stored nodes
            // For now, just note the root exists
        }
        Ok(())
    }

    pub fn state_root(&self) -> H256 {
        self.merkle_tree.root()
    }

    pub fn get_account(&self, address: &Address) -> Result<Option<Account>> {
        match self.storage.get(CF_ACCOUNTS, address.as_bytes())? {
            Some(bytes) => {
                let account: Account = bincode::deserialize(&bytes)?;
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    pub fn get_or_create_account(&self, address: &Address) -> Result<Account> {
        match self.get_account(address)? {
            Some(account) => Ok(account),
            None => Ok(Account::new(*address)),
        }
    }

    pub fn get_utxo(&self, utxo_id: &UtxoId) -> Result<Option<Utxo>> {
        let key = bincode::serialize(utxo_id)?;
        match self.storage.get(CF_UTXOS, &key)? {
            Some(bytes) => {
                let utxo: Utxo = bincode::deserialize(&bytes)?;
                Ok(Some(utxo))
            }
            None => Ok(None),
        }
    }

    pub fn apply_transaction(&mut self, tx: &Transaction) -> Result<TransactionReceipt> {
        // Validate UTxO inputs exist
        for input in &tx.inputs {
            if self.get_utxo(input)?.is_none() {
                bail!("UTxO input not found: {:?}", input);
            }
        }

        // Validate sender account
        let mut sender_account = self.get_or_create_account(&tx.sender)?;
        if sender_account.nonce != tx.nonce {
            bail!("invalid nonce: expected {}, got {}", sender_account.nonce, tx.nonce);
        }

        // Check sender has enough balance for fee
        if sender_account.balance < tx.fee {
            bail!("insufficient balance for fee");
        }

        // Deduct fee
        sender_account.balance -= tx.fee;
        sender_account.nonce += 1;

        // Process UTxO inputs (consume them)
        let mut total_input = 0u128;
        for input in &tx.inputs {
            if let Some(utxo) = self.get_utxo(input)? {
                total_input += utxo.amount;
            }
        }

        // Create new UTxOs (outputs)
        let mut total_output = 0u128;
        for output in &tx.outputs {
            total_output += output.amount;
        }

        // Validate UTxO balance
        if total_input < total_output {
            bail!("UTxO inputs insufficient for outputs");
        }

        // Apply changes
        let mut batch = StorageBatch::new();

        // Update sender account
        self.update_account_in_batch(&mut batch, sender_account)?;

        // Delete consumed UTxOs
        for input in &tx.inputs {
            let key = bincode::serialize(input)?;
            batch.delete(CF_UTXOS, key);
        }

        // Create new UTxOs
        let tx_hash = tx.hash();
        for (idx, output) in tx.outputs.iter().enumerate() {
            let utxo_id = UtxoId {
                tx_hash,
                output_index: idx as u32,
            };
            let utxo = Utxo {
                amount: output.amount,
                owner: output.owner.to_address(),
                script_hash: output.script_hash,
            };
            let key = bincode::serialize(&utxo_id)?;
            let value = bincode::serialize(&utxo)?;
            batch.put(CF_UTXOS, key, value);
        }

        // Commit batch
        self.storage.write_batch(batch)?;

        // Update Merkle tree
        self.recompute_state_root()?;

        Ok(TransactionReceipt {
            tx_hash,
            block_hash: H256::zero(), // Set by block processor
            slot: 0, // Set by block processor
            status: TransactionStatus::Success,
            gas_used: 0, // Would be computed by runtime
            logs: vec![],
            state_root: self.state_root(),
        })
    }

    fn update_account_in_batch(&self, batch: &mut StorageBatch, account: Account) -> Result<()> {
        let key = account.address.as_bytes().to_vec();
        let value = bincode::serialize(&account)?;
        batch.put(CF_ACCOUNTS, key, value);
        Ok(())
    }

    fn recompute_state_root(&mut self) -> Result<()> {
        // Iterate all accounts and update Merkle tree
        let mut accounts = HashMap::new();
        for item in self.storage.iterator(CF_ACCOUNTS)? {
            let (key_bytes, value_bytes) = item;
            if key_bytes.len() == 20 {
                let address = Address::from_slice(&key_bytes)?;
                let account: Account = bincode::deserialize(&value_bytes)?;
                let account_hash = self.hash_account(&account);
                accounts.insert(address, account_hash);
            }
        }

        // Rebuild Merkle tree
        self.merkle_tree = SparseMerkleTree::new();
        for (address, hash) in accounts {
            self.merkle_tree.update(address, hash);
        }

        // Store new root
        let root = self.merkle_tree.root();
        self.storage.put(CF_METADATA, b"state_root", root.as_bytes())?;

        Ok(())
    }

    fn hash_account(&self, account: &Account) -> H256 {
        use sha2::{Digest, Sha256};
        let bytes = bincode::serialize(account).unwrap();
        let hash = Sha256::digest(&bytes);
        H256::from_slice(&hash).unwrap()
    }

    pub fn apply_block_transactions(&mut self, transactions: &[Transaction]) -> Result<Vec<TransactionReceipt>> {
        let mut receipts = Vec::new();
        
        for tx in transactions {
            match self.apply_transaction(tx) {
                Ok(receipt) => receipts.push(receipt),
                Err(e) => {
                    // Transaction failed, still include receipt
                    receipts.push(TransactionReceipt {
                        tx_hash: tx.hash(),
                        block_hash: H256::zero(),
                        slot: 0,
                        status: TransactionStatus::Failed {
                            reason: e.to_string(),
                        },
                        gas_used: 0,
                        logs: vec![],
                        state_root: self.state_root(),
                    });
                }
            }
        }
        
        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether-crypto-primitives::Keypair;
    use aether-types::{Signature, PublicKey, UtxoOutput};
    use std::collections::HashSet;
    use tempfile::TempDir;

    #[test]
    fn test_account_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        
        let account = ledger.get_or_create_account(&address).unwrap();
        assert_eq!(account.balance, 0);
        assert_eq!(account.nonce, 0);
    }

    #[test]
    fn test_simple_transfer() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Give account some balance
        let mut account = Account::with_balance(address, 1000);
        let mut batch = StorageBatch::new();
        let key = address.as_bytes().to_vec();
        let value = bincode::serialize(&account).unwrap();
        batch.put(CF_ACCOUNTS, key, value);
        ledger.storage.write_batch(batch).unwrap();

        // Create transaction
        let tx = Transaction {
            nonce: 0,
            sender: address,
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };

        let receipt = ledger.apply_transaction(&tx).unwrap();
        assert!(matches!(receipt.status, TransactionStatus::Success));
    }
}

