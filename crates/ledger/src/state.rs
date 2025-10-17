use aether_crypto_primitives::ed25519;
use aether_state_merkle::SparseMerkleTree;
use aether_state_storage::{Storage, StorageBatch, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
use aether_types::{
    Account, Address, Transaction, TransactionReceipt, TransactionStatus, Utxo, UtxoId, H256,
};
use anyhow::{anyhow, bail, Result};
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
        if let Some(_root_bytes) = self.storage.get(CF_METADATA, b"state_root")? {
            // Rebuild Merkle tree from all accounts in storage
            // This is expensive but only happens once during initialization
            self.rebuild_merkle_tree_from_storage()?;
        }
        Ok(())
    }

    pub fn state_root(&mut self) -> H256 {
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
        tx.verify_signature()?;
        self.apply_transaction_validated(tx)
    }

    fn apply_transaction_validated(&mut self, tx: &Transaction) -> Result<TransactionReceipt> {
        // Validate UTxO inputs exist
        for input in &tx.inputs {
            if self.get_utxo(input)?.is_none() {
                bail!("UTxO input not found: {:?}", input);
            }
        }

        // Validate sender account
        let mut sender_account = self.get_or_create_account(&tx.sender)?;
        if sender_account.nonce != tx.nonce {
            bail!(
                "invalid nonce: expected {}, got {}",
                sender_account.nonce,
                tx.nonce
            );
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
        self.update_account_in_batch(&mut batch, sender_account.clone())?;

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

        // Update Merkle tree incrementally (only changed account)
        let account_hash = self.hash_account(&sender_account);
        self.merkle_tree.update(sender_account.address, account_hash);

        Ok(TransactionReceipt {
            tx_hash,
            block_hash: H256::zero(), // Set by block processor
            slot: 0,                  // Set by block processor
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

    /// Rebuild Merkle tree from storage (used during initialization/recovery)
    /// This is expensive and should only be called when loading state from disk.
    fn rebuild_merkle_tree_from_storage(&mut self) -> Result<()> {
        // Collect all accounts from storage
        let mut accounts = Vec::new();
        for item in self.storage.iterator(CF_ACCOUNTS)? {
            let (key_bytes, value_bytes) = item;
            if key_bytes.len() == 20 {
                let address = Address::from_slice(&key_bytes).map_err(|e| anyhow!(e))?;
                let account: Account = bincode::deserialize(&value_bytes)?;
                let account_hash = self.hash_account(&account);
                accounts.push((address, account_hash));
            }
        }

        // Rebuild tree with batch update (more efficient)
        self.merkle_tree = SparseMerkleTree::new();
        self.merkle_tree.batch_update(accounts);

        // Store new root
        let root = self.merkle_tree.root();
        self.storage
            .put(CF_METADATA, b"state_root", root.as_bytes())?;

        Ok(())
    }

    fn hash_account(&self, account: &Account) -> H256 {
        use sha2::{Digest, Sha256};
        let bytes = bincode::serialize(account).unwrap();
        let hash = Sha256::digest(&bytes);
        H256::from_slice(&hash).unwrap()
    }

    pub fn apply_block_transactions(
        &mut self,
        transactions: &[Transaction],
    ) -> Result<Vec<TransactionReceipt>> {
        let mut receipts = Vec::new();

        if transactions.is_empty() {
            return Ok(receipts);
        }

        let batch_inputs: Vec<_> = transactions.iter().map(|tx| tx.ed25519_tuple()).collect();
        let batch_results = ed25519::verify_batch(&batch_inputs)
            .map_err(|e| anyhow!("batch signature verification failed: {e:?}"))?;

        for (tx, is_valid) in transactions.iter().zip(batch_results.into_iter()) {
            if !is_valid {
                receipts.push(TransactionReceipt {
                    tx_hash: tx.hash(),
                    block_hash: H256::zero(),
                    slot: 0,
                    status: TransactionStatus::Failed {
                        reason: "invalid signature".to_string(),
                    },
                    gas_used: 0,
                    logs: vec![],
                    state_root: self.state_root(),
                });
                continue;
            }

            match self.apply_transaction_validated(tx) {
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
    use aether_crypto_primitives::Keypair;
    use aether_types::{PublicKey, Signature};
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
        let account = Account::with_balance(address, 1000);
        let mut batch = StorageBatch::new();
        let key = address.as_bytes().to_vec();
        let value = bincode::serialize(&account).unwrap();
        batch.put(CF_ACCOUNTS, key, value);
        ledger.storage.write_batch(batch).unwrap();

        // Create transaction
        let mut tx = Transaction {
            nonce: 0,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
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
        let hash = tx.hash();
        let signature = keypair.sign(hash.as_bytes());
        tx.signature = Signature::from_bytes(signature);

        let receipt = ledger.apply_transaction(&tx).unwrap();
        assert!(matches!(receipt.status, TransactionStatus::Success));
    }

    #[test]
    fn batch_verification_marks_invalid_signatures() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Seed balance
        let account = Account::with_balance(address, 1_000);
        let mut batch = StorageBatch::new();
        let key = address.as_bytes().to_vec();
        let value = bincode::serialize(&account).unwrap();
        batch.put(CF_ACCOUNTS, key, value);
        ledger.storage.write_batch(batch).unwrap();

        // Build signed transaction
        let mut tx = Transaction {
            nonce: 0,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        let signature = keypair.sign(hash.as_bytes());
        tx.signature = Signature::from_bytes(signature.clone());

        let mut invalid_tx = tx.clone();
        invalid_tx.signature = Signature::from_bytes(vec![0; 64]);

        let receipts = ledger
            .apply_block_transactions(&[tx.clone(), invalid_tx])
            .unwrap();

        assert_eq!(receipts.len(), 2);
        assert!(matches!(receipts[0].status, TransactionStatus::Success));
        assert!(matches!(
            receipts[1].status,
            TransactionStatus::Failed { .. }
        ));
        if let TransactionStatus::Failed { reason } = &receipts[1].status {
            assert!(reason.contains("invalid signature"));
        }
    }
}
