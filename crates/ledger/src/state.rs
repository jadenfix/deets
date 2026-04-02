use aether_crypto_primitives::ed25519;
use aether_state_merkle::SparseMerkleTree;
use aether_state_storage::{Storage, StorageBatch, CF_ACCOUNTS, CF_METADATA, CF_UTXOS};
use aether_types::{
    Account, Address, Transaction, TransactionReceipt, TransactionStatus, TransferPayload, Utxo,
    UtxoId, H256, TRANSFER_PROGRAM_ID,
};
use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, HashSet};

/// In-memory overlay for speculative block execution.
/// Reads check overlay first, falls back to storage. Writes stay in memory
/// until explicitly committed via `commit_overlay()`.
#[derive(Debug)]
pub struct PendingOverlay {
    pub writes: HashMap<(String, Vec<u8>), Vec<u8>>,
    pub deletes: HashSet<(String, Vec<u8>)>,
    pub changed_accounts: Vec<Address>,
    pub state_root: H256,
}

impl PendingOverlay {
    fn new() -> Self {
        PendingOverlay {
            writes: HashMap::new(),
            deletes: HashSet::new(),
            changed_accounts: Vec::new(),
            state_root: H256::zero(),
        }
    }

    fn put(&mut self, cf: &str, key: Vec<u8>, value: Vec<u8>) {
        self.deletes.remove(&(cf.to_string(), key.clone()));
        self.writes.insert((cf.to_string(), key), value);
    }

    fn delete(&mut self, cf: &str, key: Vec<u8>) {
        self.writes.remove(&(cf.to_string(), key.clone()));
        self.deletes.insert((cf.to_string(), key));
    }

    fn get(&self, cf: &str, key: &[u8]) -> Option<Option<Vec<u8>>> {
        let map_key = (cf.to_string(), key.to_vec());
        if self.deletes.contains(&map_key) {
            return Some(None); // Deleted in overlay
        }
        self.writes.get(&map_key).map(|v| Some(v.clone()))
    }
}

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
        // Always rebuild Merkle tree from accounts on startup.
        // This handles both normal restart (metadata exists) and recovery
        // (metadata missing but accounts exist in storage).
        let has_metadata = self.storage.get(CF_METADATA, b"state_root")?.is_some();
        let has_accounts = self.storage.iterator(CF_ACCOUNTS)?.next().is_some();

        if has_metadata || has_accounts {
            self.recompute_state_root()?;
        }
        Ok(())
    }

    pub fn state_root(&self) -> H256 {
        self.merkle_tree.root()
    }

    /// Access the underlying storage (for block/receipt persistence).
    pub fn storage(&self) -> &Storage {
        &self.storage
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
        // Reject duplicate UTxO inputs within a single transaction.
        // Without this, an attacker lists the same input twice, counting its value
        // double and effectively minting tokens out of thin air.
        {
            let mut seen_inputs = HashSet::new();
            for input in &tx.inputs {
                if !seen_inputs.insert(input) {
                    bail!("duplicate UTxO input in transaction: {:?}", input);
                }
            }
        }

        // Validate UTxO inputs: existence, ownership, and accumulate total in one pass
        let mut total_input = 0u128;
        for input in &tx.inputs {
            let utxo = self
                .get_utxo(input)?
                .ok_or_else(|| anyhow!("UTxO input not found: {:?}", input))?;
            if utxo.owner != tx.sender {
                bail!(
                    "UTxO input {:?} owned by {:?}, not sender {:?}",
                    input,
                    utxo.owner,
                    tx.sender
                );
            }
            total_input = total_input
                .checked_add(utxo.amount)
                .ok_or_else(|| anyhow!("UTxO total input overflow"))?;
        }

        let transfer_payload = self.decode_transfer_payload(tx)?;
        if transfer_payload.is_some() && (!tx.inputs.is_empty() || !tx.outputs.is_empty()) {
            bail!("transfer program transactions cannot mix UTxO inputs/outputs");
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

        let is_utxo_tx = !tx.inputs.is_empty() || !tx.outputs.is_empty();

        // For UTxO transactions, the fee is paid from the UTxO surplus
        // (total_input - total_output >= fee). For account/transfer transactions,
        // the fee is deducted from the sender's account balance.
        if !is_utxo_tx {
            let transfer_amount =
                transfer_payload.as_ref().map(|p| p.amount).unwrap_or(0);
            let total_debit = tx
                .fee
                .checked_add(transfer_amount)
                .ok_or_else(|| anyhow!("fee + transfer amount overflow"))?;
            if sender_account.balance < total_debit {
                bail!("insufficient balance for fee and transfer amount");
            }
            sender_account.balance = sender_account
                .balance
                .checked_sub(total_debit)
                .ok_or_else(|| anyhow!("balance underflow during debit"))?;
        }

        sender_account.nonce = sender_account
            .nonce
            .checked_add(1)
            .ok_or_else(|| anyhow!("nonce overflow"))?;

        // Track which accounts changed for incremental Merkle update
        let mut changed_accounts = vec![tx.sender];

        let mut recipient_account: Option<Account> = None;
        if let Some(payload) = &transfer_payload {
            if payload.recipient == tx.sender {
                sender_account.balance = sender_account
                    .balance
                    .checked_add(payload.amount)
                    .ok_or_else(|| anyhow!("sender balance overflow"))?;
            } else {
                let mut recipient = self.get_or_create_account(&payload.recipient)?;
                recipient.balance = recipient
                    .balance
                    .checked_add(payload.amount)
                    .ok_or_else(|| anyhow!("recipient balance overflow"))?;
                changed_accounts.push(payload.recipient);
                recipient_account = Some(recipient);
            }
        }

        // Create new UTxOs (outputs)
        let mut total_output = 0u128;
        for output in &tx.outputs {
            total_output = total_output
                .checked_add(output.amount)
                .ok_or_else(|| anyhow!("UTxO total output overflow"))?;
        }

        // Validate UTxO balance: inputs must cover outputs + fee
        if is_utxo_tx {
            let required = total_output
                .checked_add(tx.fee)
                .ok_or_else(|| anyhow!("UTxO output + fee overflow"))?;
            if total_input < required {
                bail!("UTxO inputs insufficient for outputs + fee");
            }
        } else if total_input < total_output {
            bail!("UTxO inputs insufficient for outputs");
        }

        // Apply changes
        let mut batch = StorageBatch::new();

        // Update sender account
        self.update_account_in_batch(&mut batch, sender_account.clone())?;
        if let Some(ref account) = recipient_account {
            self.update_account_in_batch(&mut batch, account.clone())?;
        }

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

        // Incremental Merkle update — include state_root in the same batch
        self.update_state_root_incremental(
            &sender_account,
            recipient_account.as_ref(),
            Some(&mut batch),
        )?;

        // Commit everything atomically in a single WriteBatch
        self.storage.write_batch(batch)?;

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

    fn decode_transfer_payload(&self, tx: &Transaction) -> Result<Option<TransferPayload>> {
        if tx.program_id != Some(TRANSFER_PROGRAM_ID) {
            return Ok(None);
        }
        if tx.data.is_empty() {
            bail!("transfer program payload is empty");
        }

        let payload: TransferPayload = bincode::deserialize(&tx.data)
            .map_err(|e| anyhow!("invalid transfer payload encoding: {e}"))?;
        if payload.amount == 0 {
            bail!("transfer amount must be greater than zero");
        }

        Ok(Some(payload))
    }

    fn update_account_in_batch(&self, batch: &mut StorageBatch, account: Account) -> Result<()> {
        let key = account.address.as_bytes().to_vec();
        let value = bincode::serialize(&account)?;
        batch.put(CF_ACCOUNTS, key, value);
        Ok(())
    }

    /// Incrementally update the Merkle tree for changed accounts only.
    /// This is O(k) where k = number of changed accounts, instead of O(n) for all accounts.
    ///
    /// If `batch` is provided, the state root is written into the batch for atomic
    /// commit with other state changes. Otherwise, the root is written directly.
    fn update_state_root_incremental(
        &mut self,
        sender: &Account,
        recipient: Option<&Account>,
        batch: Option<&mut StorageBatch>,
    ) -> Result<()> {
        // Update sender leaf
        let sender_hash = self.hash_account(sender);
        self.merkle_tree.update(sender.address, sender_hash);

        // Update recipient leaf (if different from sender)
        if let Some(recipient) = recipient {
            let recipient_hash = self.hash_account(recipient);
            self.merkle_tree.update(recipient.address, recipient_hash);
        }

        // Persist the new root — either in the batch (atomic) or directly
        let root = self.merkle_tree.root();
        if let Some(batch) = batch {
            batch.put(
                CF_METADATA,
                b"state_root".to_vec(),
                root.as_bytes().to_vec(),
            );
        } else {
            self.storage
                .put(CF_METADATA, b"state_root", root.as_bytes())?;
        }

        Ok(())
    }

    /// Full rebuild of state root from all accounts (used on startup/recovery).
    /// Safe because this runs during initialization before any concurrent access.
    fn recompute_state_root(&mut self) -> Result<()> {
        let mut accounts = HashMap::new();
        for item in self.storage.iterator(CF_ACCOUNTS)? {
            let (key_bytes, value_bytes) = item;
            if key_bytes.len() == 20 {
                let address = Address::from_slice(&key_bytes).map_err(|e| anyhow!(e))?;
                let account: Account = bincode::deserialize(&value_bytes)?;
                let account_hash = self.hash_account(&account);
                accounts.insert(address, account_hash);
            }
        }

        self.merkle_tree = SparseMerkleTree::new();
        for (address, hash) in accounts {
            self.merkle_tree.update(address, hash);
        }

        // Persist recomputed root atomically
        let root = self.merkle_tree.root();
        let mut batch = StorageBatch::new();
        batch.put(
            CF_METADATA,
            b"state_root".to_vec(),
            root.as_bytes().to_vec(),
        );
        self.storage.write_batch(batch)?;

        Ok(())
    }

    fn hash_account(&self, account: &Account) -> H256 {
        use sha2::{Digest, Sha256};
        let bytes = bincode::serialize(account).expect("Account serialization infallible");
        let hash = Sha256::digest(&bytes);
        H256::from_slice(&hash).expect("SHA256 produces 32 bytes")
    }

    /// Record burned fees in ledger metadata. This permanently removes tokens from
    /// circulation, implementing EIP-1559 deflationary pressure.
    pub fn record_burned_fees(&mut self, amount: u128) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let current = self
            .storage
            .get(CF_METADATA, b"total_burned")?
            .map(|bytes| {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes[..16.min(bytes.len())]);
                u128::from_le_bytes(arr)
            })
            .unwrap_or(0);
        let new_total = current.saturating_add(amount);
        self.storage
            .put(CF_METADATA, b"total_burned", &new_total.to_le_bytes())?;
        Ok(())
    }

    /// Fold proposer reward and burn accounting into an existing write batch,
    /// making fee distribution atomic with the overlay commit.
    ///
    /// Must be called AFTER `prepare_overlay_batch` (so the in-memory Merkle tree
    /// already reflects the overlay state) and BEFORE the batch is written to disk.
    ///
    /// Uses `overlay` to read the proposer's post-transaction balance in case the
    /// proposer also submitted transactions in this block.
    pub fn fold_fee_distribution_into_batch(
        &mut self,
        batch: &mut StorageBatch,
        overlay: &PendingOverlay,
        proposer: &Address,
        proposer_reward: u128,
        burned: u128,
    ) -> Result<()> {
        if proposer_reward > 0 {
            // Read proposer account from overlay first (in case proposer submitted txs),
            // falling back to DB if not present in the overlay.
            let mut account = self.get_account_from_overlay(overlay, proposer)?;
            account.balance = account
                .balance
                .checked_add(proposer_reward)
                .ok_or_else(|| anyhow!("proposer balance overflow"))?;
            self.update_account_in_batch(batch, account.clone())?;
            // Overwrites the state_root entry already in the batch with the updated root.
            self.update_state_root_incremental(&account, None, Some(batch))?;
        }

        if burned > 0 {
            let current = self
                .storage
                .get(CF_METADATA, b"total_burned")?
                .map(|bytes| {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&bytes[..16.min(bytes.len())]);
                    u128::from_le_bytes(arr)
                })
                .unwrap_or(0);
            let new_total = current.saturating_add(burned);
            batch.put(
                CF_METADATA,
                b"total_burned".to_vec(),
                new_total.to_le_bytes().to_vec(),
            );
        }

        Ok(())
    }

    /// Get the total amount of fees burned since genesis.
    pub fn total_burned(&self) -> u128 {
        self.storage
            .get(CF_METADATA, b"total_burned")
            .ok()
            .flatten()
            .map(|bytes| {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes[..16.min(bytes.len())]);
                u128::from_le_bytes(arr)
            })
            .unwrap_or(0)
    }

    /// Credit an account with a reward (for epoch emissions, proposer rewards).
    pub fn credit_account(&mut self, address: &Address, amount: u128) -> Result<()> {
        let mut account = self.get_or_create_account(address)?;
        account.balance = account
            .balance
            .checked_add(amount)
            .ok_or_else(|| anyhow!("balance overflow crediting account"))?;

        let mut batch = StorageBatch::new();
        self.update_account_in_batch(&mut batch, account.clone())?;
        self.update_state_root_incremental(&account, None, Some(&mut batch))?;
        self.storage.write_batch(batch)?;
        Ok(())
    }

    /// Save a snapshot of the current state root for potential rollback.
    pub fn snapshot_state_root(&self) -> H256 {
        self.merkle_tree.root()
    }

    /// Execute a block's transactions speculatively without committing to storage.
    /// Returns receipts, the computed state root, and a PendingOverlay that can be
    /// committed or discarded.
    pub fn apply_block_speculatively(
        &mut self,
        transactions: &[Transaction],
    ) -> Result<(Vec<TransactionReceipt>, PendingOverlay)> {
        self.apply_block_speculatively_with_chain_id(transactions, None)
    }

    /// Execute a block's transactions speculatively, optionally validating chain_id.
    pub fn apply_block_speculatively_with_chain_id(
        &mut self,
        transactions: &[Transaction],
        expected_chain_id: Option<u64>,
    ) -> Result<(Vec<TransactionReceipt>, PendingOverlay)> {
        let mut overlay = PendingOverlay::new();
        let mut receipts = Vec::new();

        if transactions.is_empty() {
            overlay.state_root = self.state_root();
            return Ok((receipts, overlay));
        }

        let batch_inputs: Vec<_> = transactions.iter().map(|tx| tx.ed25519_tuple()).collect();
        let batch_results = ed25519::verify_batch(&batch_inputs)
            .map_err(|e| anyhow!("batch signature verification failed: {e:?}"))?;

        // Clone the merkle tree for speculative root computation
        let mut spec_tree = self.merkle_tree.clone();

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
                    state_root: spec_tree.root(),
                });
                continue;
            }

            // Validate chain_id to prevent cross-chain replay attacks
            if let Some(expected_id) = expected_chain_id {
                if tx.chain_id != expected_id {
                    receipts.push(TransactionReceipt {
                        tx_hash: tx.hash(),
                        block_hash: H256::zero(),
                        slot: 0,
                        status: TransactionStatus::Failed {
                            reason: format!(
                                "wrong chain_id: expected {}, got {}",
                                expected_id, tx.chain_id
                            ),
                        },
                        gas_used: 0,
                        logs: vec![],
                        state_root: spec_tree.root(),
                    });
                    continue;
                }
            }

            match self.apply_tx_to_overlay(tx, &mut overlay, &mut spec_tree) {
                Ok(receipt) => receipts.push(receipt),
                Err(e) => {
                    receipts.push(TransactionReceipt {
                        tx_hash: tx.hash(),
                        block_hash: H256::zero(),
                        slot: 0,
                        status: TransactionStatus::Failed {
                            reason: e.to_string(),
                        },
                        gas_used: 0,
                        logs: vec![],
                        state_root: spec_tree.root(),
                    });
                }
            }
        }

        overlay.state_root = spec_tree.root();
        Ok((receipts, overlay))
    }

    /// Apply a single transaction to the overlay (not to disk).
    fn apply_tx_to_overlay(
        &self,
        tx: &Transaction,
        overlay: &mut PendingOverlay,
        spec_tree: &mut SparseMerkleTree,
    ) -> Result<TransactionReceipt> {
        // Read sender account from overlay first, then storage
        let mut sender_account = self.get_account_from_overlay(overlay, &tx.sender)?;
        if sender_account.nonce != tx.nonce {
            bail!(
                "invalid nonce: expected {}, got {}",
                sender_account.nonce,
                tx.nonce
            );
        }

        let transfer_payload = self.decode_transfer_payload(tx)?;
        if transfer_payload.is_some() && (!tx.inputs.is_empty() || !tx.outputs.is_empty()) {
            bail!("transfer program transactions cannot mix UTxO inputs/outputs");
        }

        let is_utxo_tx = !tx.inputs.is_empty() || !tx.outputs.is_empty();

        // For UTxO transactions, the fee is paid from the UTxO surplus
        // (total_input - total_output >= fee). For account/transfer transactions,
        // the fee is deducted from the sender's account balance.
        if !is_utxo_tx {
            let transfer_amount =
                transfer_payload.as_ref().map(|p| p.amount).unwrap_or(0);
            let total_debit = tx
                .fee
                .checked_add(transfer_amount)
                .ok_or_else(|| anyhow!("fee + transfer amount overflow"))?;
            if sender_account.balance < total_debit {
                bail!("insufficient balance for fee and transfer amount");
            }
            sender_account.balance = sender_account
                .balance
                .checked_sub(total_debit)
                .ok_or_else(|| anyhow!("balance underflow during debit"))?;
        }

        sender_account.nonce = sender_account
            .nonce
            .checked_add(1)
            .ok_or_else(|| anyhow!("nonce overflow"))?;

        let mut recipient_account: Option<Account> = None;
        if let Some(payload) = &transfer_payload {
            if payload.recipient == tx.sender {
                sender_account.balance = sender_account
                    .balance
                    .checked_add(payload.amount)
                    .ok_or_else(|| anyhow!("sender balance overflow"))?;
            } else {
                let mut recipient = self.get_account_from_overlay(overlay, &payload.recipient)?;
                recipient.balance = recipient
                    .balance
                    .checked_add(payload.amount)
                    .ok_or_else(|| anyhow!("recipient balance overflow"))?;
                recipient_account = Some(recipient);
            }
        }

        // Write to overlay (NOT to disk)
        let sender_bytes = bincode::serialize(&sender_account)?;
        overlay.put(CF_ACCOUNTS, tx.sender.as_bytes().to_vec(), sender_bytes);
        overlay.changed_accounts.push(tx.sender);

        // Update speculative merkle tree
        let sender_hash = self.hash_account(&sender_account);
        spec_tree.update(sender_account.address, sender_hash);

        if let Some(ref recipient) = recipient_account {
            let recipient_bytes = bincode::serialize(recipient)?;
            overlay.put(
                CF_ACCOUNTS,
                recipient.address.as_bytes().to_vec(),
                recipient_bytes,
            );
            overlay.changed_accounts.push(recipient.address);

            let recipient_hash = self.hash_account(recipient);
            spec_tree.update(recipient.address, recipient_hash);
        }

        // Reject duplicate UTxO inputs within a single transaction.
        // Without this check, an attacker could list the same input twice,
        // counting its value double and effectively minting tokens.
        {
            let mut seen_inputs = HashSet::new();
            for input in &tx.inputs {
                if !seen_inputs.insert(input) {
                    bail!("duplicate UTxO input in transaction: {:?}", input);
                }
            }
        }

        // Validate UTxO inputs: existence, ownership, and accumulate total
        let mut total_input = 0u128;
        for input in &tx.inputs {
            let key = bincode::serialize(input)?;
            let utxo: Utxo = match overlay.get(CF_UTXOS, &key) {
                Some(Some(bytes)) => bincode::deserialize(&bytes)?,
                Some(None) => bail!("UTxO input already spent in this block: {:?}", input),
                None => {
                    let stored = self
                        .get_utxo(input)?
                        .ok_or_else(|| anyhow!("UTxO input not found: {:?}", input))?;
                    stored
                }
            };
            if utxo.owner != tx.sender {
                bail!(
                    "UTxO input {:?} owned by {:?}, not sender {:?}",
                    input,
                    utxo.owner,
                    tx.sender
                );
            }
            total_input = total_input
                .checked_add(utxo.amount)
                .ok_or_else(|| anyhow!("UTxO total input overflow"))?;
        }

        // Validate UTxO outputs and balance
        let mut total_output = 0u128;
        for output in &tx.outputs {
            total_output = total_output
                .checked_add(output.amount)
                .ok_or_else(|| anyhow!("UTxO total output overflow"))?;
        }
        // Validate UTxO balance: inputs must cover outputs + fee
        if is_utxo_tx {
            let required = total_output
                .checked_add(tx.fee)
                .ok_or_else(|| anyhow!("UTxO output + fee overflow"))?;
            if total_input < required {
                bail!("UTxO inputs insufficient for outputs + fee");
            }
        } else if total_input < total_output {
            bail!("UTxO inputs insufficient for outputs");
        }

        // Delete consumed UTxOs from overlay
        for input in &tx.inputs {
            let key = bincode::serialize(input)?;
            overlay.delete(CF_UTXOS, key);
        }
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
            overlay.put(CF_UTXOS, key, value);
        }

        Ok(TransactionReceipt {
            tx_hash,
            block_hash: H256::zero(),
            slot: 0,
            status: TransactionStatus::Success,
            gas_used: 0,
            logs: vec![],
            state_root: spec_tree.root(),
        })
    }

    /// Read an account from overlay first, then fall back to storage.
    fn get_account_from_overlay(
        &self,
        overlay: &PendingOverlay,
        address: &Address,
    ) -> Result<Account> {
        if let Some(maybe_bytes) = overlay.get(CF_ACCOUNTS, address.as_bytes()) {
            match maybe_bytes {
                Some(bytes) => Ok(bincode::deserialize(&bytes)?),
                None => Ok(Account::new(*address)), // Deleted in overlay → new account
            }
        } else {
            self.get_or_create_account(address)
        }
    }

    /// Build a StorageBatch from a speculative overlay WITHOUT writing to disk.
    /// Updates the in-memory merkle tree and includes the state root in the batch.
    /// The caller can extend this batch with additional data (e.g. block/receipt
    /// persistence) before writing, ensuring a single atomic commit.
    pub fn prepare_overlay_batch(&mut self, overlay: &PendingOverlay) -> Result<StorageBatch> {
        let mut batch = StorageBatch::new();
        for ((cf, key), value) in &overlay.writes {
            batch.put(cf, key.clone(), value.clone());
        }
        for (cf, key) in &overlay.deletes {
            batch.delete(cf, key.clone());
        }

        // Update merkle tree with changed accounts
        for addr in &overlay.changed_accounts {
            if let Some(maybe_bytes) = overlay.get(CF_ACCOUNTS, addr.as_bytes()) {
                match maybe_bytes {
                    Some(bytes) => {
                        let account: Account = bincode::deserialize(&bytes)?;
                        let hash = self.hash_account(&account);
                        self.merkle_tree.update(*addr, hash);
                    }
                    None => {
                        self.merkle_tree.update(*addr, H256::zero());
                    }
                }
            }
        }

        // Include state root in the batch
        let root = self.merkle_tree.root();
        batch.put(
            CF_METADATA,
            b"state_root".to_vec(),
            root.as_bytes().to_vec(),
        );

        Ok(batch)
    }

    /// Commit a speculative overlay to permanent storage.
    /// All state changes (accounts, UTXOs, state root) are written in a single
    /// atomic WriteBatch so a crash mid-commit cannot corrupt state.
    pub fn commit_overlay(&mut self, overlay: PendingOverlay) -> Result<()> {
        let batch = self.prepare_overlay_batch(&overlay)?;
        self.storage.write_batch(batch)?;
        Ok(())
    }

    /// Write a pre-built StorageBatch to disk.
    /// Used when the caller has combined multiple logical operations (e.g. overlay
    /// commit + block persistence) into a single atomic batch.
    pub fn write_batch(&self, batch: StorageBatch) -> Result<()> {
        self.storage.write_batch(batch)
    }

    pub fn seed_account(&mut self, address: &Address, balance: u128) -> Result<()> {
        let account = Account::with_balance(*address, balance);
        let mut batch = StorageBatch::new();
        let key = address.as_bytes().to_vec();
        let value = bincode::serialize(&account)?;
        batch.put(CF_ACCOUNTS, key, value);
        // Include state root in same atomic batch
        self.update_state_root_incremental(&account, None, Some(&mut batch))?;
        self.storage.write_batch(batch)?;
        Ok(())
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
    use aether_types::{PublicKey, Signature, TransferPayload, TRANSFER_PROGRAM_ID};
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
            chain_id: 1,
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
    fn test_batch_verification_marks_invalid_signatures() {
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
            chain_id: 1,
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

    #[test]
    fn test_transfer_program_moves_balance() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let sender_key = Keypair::generate();
        let sender = Address::from_slice(&sender_key.to_address()).unwrap();
        let recipient = Address::from_slice(&[9u8; 20]).unwrap();

        let mut seed_batch = StorageBatch::new();
        seed_batch.put(
            CF_ACCOUNTS,
            sender.as_bytes().to_vec(),
            bincode::serialize(&Account::with_balance(sender, 100_000)).unwrap(),
        );
        ledger.storage.write_batch(seed_batch).unwrap();

        let payload = TransferPayload {
            recipient,
            amount: 1_500,
            memo: Some("ledger test".to_string()),
        };
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender,
            sender_pubkey: PublicKey::from_bytes(sender_key.public_key()),
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: Some(TRANSFER_PROGRAM_ID),
            data: bincode::serialize(&payload).unwrap(),
            gas_limit: 21_000,
            fee: 400,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(sender_key.sign(hash.as_bytes()));

        let receipt = ledger.apply_transaction(&tx).unwrap();
        assert!(matches!(receipt.status, TransactionStatus::Success));

        let sender_after = ledger.get_account(&sender).unwrap().unwrap();
        let recipient_after = ledger.get_account(&recipient).unwrap().unwrap();
        assert_eq!(sender_after.nonce, 1);
        assert_eq!(sender_after.balance, 98_100);
        assert_eq!(recipient_after.balance, 1_500);
    }

    #[test]
    fn test_state_root_persisted_atomically_with_accounts() {
        // Verify that after a transaction, the state root stored in metadata
        // matches the in-memory merkle tree — proving they were written together.
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        ledger.seed_account(&address, 10_000).unwrap();

        let root_after_seed = ledger.state_root();
        let stored_root = ledger
            .storage()
            .get(CF_METADATA, b"state_root")
            .unwrap()
            .map(|b| H256::from_slice(&b).unwrap());
        assert_eq!(Some(root_after_seed), stored_root);

        // Apply a transaction and verify consistency again
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
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
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
        ledger.apply_transaction(&tx).unwrap();

        let root_after_tx = ledger.state_root();
        let stored_root2 = ledger
            .storage()
            .get(CF_METADATA, b"state_root")
            .unwrap()
            .map(|b| H256::from_slice(&b).unwrap());
        assert_eq!(Some(root_after_tx), stored_root2);
        assert_ne!(root_after_seed, root_after_tx);
    }

    #[test]
    fn test_overlay_commit_atomic_state_root() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        ledger.seed_account(&address, 50_000).unwrap();

        // Build and commit an overlay
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 200,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let (receipts, overlay) = ledger.apply_block_speculatively(&[tx]).unwrap();
        assert!(matches!(receipts[0].status, TransactionStatus::Success));

        let overlay_root = overlay.state_root;
        ledger.commit_overlay(overlay).unwrap();

        // After commit, stored root must match overlay's computed root
        let stored_root = ledger
            .storage()
            .get(CF_METADATA, b"state_root")
            .unwrap()
            .map(|b| H256::from_slice(&b).unwrap());
        assert_eq!(Some(overlay_root), stored_root);
        assert_eq!(ledger.state_root(), overlay_root);
    }

    #[test]
    fn test_prepare_overlay_batch_and_extend() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        ledger.seed_account(&address, 50_000).unwrap();

        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 200,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let (_receipts, overlay) = ledger.apply_block_speculatively(&[tx]).unwrap();
        let overlay_root = overlay.state_root;

        // Build overlay batch WITHOUT writing
        let mut batch = ledger.prepare_overlay_batch(&overlay).unwrap();

        // Nothing should be persisted yet — state_root in DB is still the seed root
        let root_before = ledger
            .storage()
            .get(CF_METADATA, b"state_root")
            .unwrap()
            .map(|b| H256::from_slice(&b).unwrap());
        assert_ne!(root_before, Some(overlay_root), "batch should not be written yet");

        // Extend with extra data (simulating block persistence)
        let mut extra = StorageBatch::new();
        extra.put(CF_METADATA, b"test_key".to_vec(), b"test_value".to_vec());
        batch.extend(extra);

        // Single atomic write
        ledger.write_batch(batch).unwrap();

        // Now both overlay state AND extra data should be persisted
        let stored_root = ledger
            .storage()
            .get(CF_METADATA, b"state_root")
            .unwrap()
            .map(|b| H256::from_slice(&b).unwrap());
        assert_eq!(Some(overlay_root), stored_root);

        let extra_val = ledger.storage().get(CF_METADATA, b"test_key").unwrap();
        assert_eq!(extra_val, Some(b"test_value".to_vec()));
    }

    #[test]
    fn test_nonce_replay_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Seed balance
        let account = Account::with_balance(address, 10_000);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // First tx with nonce 0 succeeds
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
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
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let receipt = ledger.apply_transaction(&tx).unwrap();
        assert!(matches!(receipt.status, TransactionStatus::Success));

        // Replay same tx (nonce 0 again) must fail
        let err = ledger.apply_transaction(&tx).unwrap_err();
        assert!(
            err.to_string().contains("invalid nonce"),
            "replay should be rejected with nonce error, got: {}",
            err
        );
    }

    #[test]
    fn test_nonce_must_be_sequential() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        let account = Account::with_balance(address, 10_000);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // Skip nonce 0, try nonce 1 — must fail
        let mut tx = Transaction {
            nonce: 1,
            chain_id: 1,
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
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let err = ledger.apply_transaction(&tx).unwrap_err();
        assert!(
            err.to_string().contains("invalid nonce"),
            "out-of-order nonce should be rejected, got: {}",
            err
        );
    }

    #[test]
    fn test_cross_chain_replay_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        let account = Account::with_balance(address, 10_000);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // Tx with chain_id=999 (wrong chain)
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 999,
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
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        // With chain_id enforcement, tx should be rejected
        let (receipts, _overlay) = ledger
            .apply_block_speculatively_with_chain_id(&[tx], Some(900))
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { reason } if reason.contains("wrong chain_id")),
            "cross-chain tx should be rejected, got: {:?}",
            receipts[0].status
        );
    }

    #[test]
    fn test_intra_block_nonce_replay_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        let account = Account::with_balance(address, 10_000);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // Two identical transactions in the same block (same nonce)
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
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
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let (receipts, _overlay) = ledger
            .apply_block_speculatively(&[tx.clone(), tx])
            .unwrap();
        assert_eq!(receipts.len(), 2);
        assert!(matches!(receipts[0].status, TransactionStatus::Success));
        assert!(
            matches!(&receipts[1].status, TransactionStatus::Failed { reason } if reason.contains("invalid nonce")),
            "duplicate nonce in same block should be rejected, got: {:?}",
            receipts[1].status
        );
    }

    #[test]
    fn test_utxo_fee_not_double_charged() {
        // Regression test: UTxO transactions should pay fees from the UTxO surplus
        // (total_input - total_output), NOT from the sender's account balance.
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Give account balance=500 and a UTxO worth 1000
        ledger.seed_account(&address, 500).unwrap();

        // Create a UTxO in storage
        let fake_tx_hash = H256::from_slice(&[0xAA; 32]).unwrap();
        let utxo_id = UtxoId {
            tx_hash: fake_tx_hash,
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 1000,
            owner: address,
            script_hash: None,
        };
        let mut batch = StorageBatch::new();
        let key = bincode::serialize(&utxo_id).unwrap();
        let value = bincode::serialize(&utxo).unwrap();
        batch.put(CF_UTXOS, key, value);
        ledger.storage.write_batch(batch).unwrap();

        // Spend the UTxO: 1000 in, 900 out, fee=100. Surplus covers fee exactly.
        let recipient_kp = Keypair::generate();
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id],
            outputs: vec![aether_types::UtxoOutput {
                amount: 900,
                owner: PublicKey::from_bytes(recipient_kp.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let receipt = ledger.apply_transaction(&tx).unwrap();
        assert!(
            matches!(receipt.status, TransactionStatus::Success),
            "UTxO tx should succeed; got: {:?}",
            receipt.status
        );

        // Account balance should be UNCHANGED (fee came from UTxO surplus, not account)
        let sender = ledger.get_account(&address).unwrap().unwrap();
        assert_eq!(
            sender.balance, 500,
            "account balance must not be debited for UTxO tx fee"
        );
        assert_eq!(sender.nonce, 1);
    }

    #[test]
    fn test_utxo_insufficient_surplus_for_fee() {
        // UTxO tx where input - output < fee should be rejected
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        ledger.seed_account(&address, 10_000).unwrap();

        let fake_tx_hash = H256::from_slice(&[0xBB; 32]).unwrap();
        let utxo_id = UtxoId {
            tx_hash: fake_tx_hash,
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 1000,
            owner: address,
            script_hash: None,
        };
        let mut batch = StorageBatch::new();
        let key = bincode::serialize(&utxo_id).unwrap();
        let value = bincode::serialize(&utxo).unwrap();
        batch.put(CF_UTXOS, key, value);
        ledger.storage.write_batch(batch).unwrap();

        // 1000 in, 950 out, fee=100 => surplus=50 < fee=100 => should fail
        let recipient_kp = Keypair::generate();
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id],
            outputs: vec![aether_types::UtxoOutput {
                amount: 950,
                owner: PublicKey::from_bytes(recipient_kp.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let result = ledger.apply_transaction(&tx);
        assert!(
            result.is_err(),
            "UTxO tx with insufficient surplus for fee should fail"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("insufficient for outputs + fee"),
            "expected fee-related error, got: {}",
            err
        );
    }

    #[test]
    fn test_utxo_fee_in_speculative_path() {
        // Same double-charge fix verified through the speculative (overlay) path
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        ledger.seed_account(&address, 500).unwrap();

        let fake_tx_hash = H256::from_slice(&[0xCC; 32]).unwrap();
        let utxo_id = UtxoId {
            tx_hash: fake_tx_hash,
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 1000,
            owner: address,
            script_hash: None,
        };
        let mut batch = StorageBatch::new();
        let key = bincode::serialize(&utxo_id).unwrap();
        let value = bincode::serialize(&utxo).unwrap();
        batch.put(CF_UTXOS, key, value);
        ledger.storage.write_batch(batch).unwrap();

        let recipient_kp = Keypair::generate();
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id],
            outputs: vec![aether_types::UtxoOutput {
                amount: 900,
                owner: PublicKey::from_bytes(recipient_kp.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let (receipts, _overlay) = ledger.apply_block_speculatively(&[tx]).unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(
            matches!(receipts[0].status, TransactionStatus::Success),
            "speculative UTxO tx should succeed; got: {:?}",
            receipts[0].status
        );
    }

    #[test]
    fn test_overlay_rejects_transfer_with_utxo_mixing() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();
        ledger.seed_account(&address, 1_000_000).unwrap();

        let recipient_keypair = Keypair::generate();
        let recipient = Address::from_slice(&recipient_keypair.to_address()).unwrap();

        let payload = TransferPayload {
            recipient,
            amount: 1_000,
            memo: None,
        };
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![],
            outputs: vec![aether_types::UtxoOutput {
                amount: 999_999,
                owner: PublicKey::from_bytes(keypair.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: Some(TRANSFER_PROGRAM_ID),
            data: bincode::serialize(&payload).unwrap(),
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let (receipts, _overlay) =
            ledger.apply_block_speculatively(&[tx]).unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { reason } if reason.contains("cannot mix")),
            "transfer+UTxO mixing should be rejected in overlay path, got: {:?}",
            receipts[0].status
        );
    }

    /// Regression test: a transaction that lists the same UTxO input twice
    /// should be rejected, not count the input value double.
    #[test]
    fn test_duplicate_utxo_input_rejected_apply_transaction() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Seed account for nonce tracking
        let account = Account::with_balance(address, 0);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );

        // Create a UTxO worth 500
        let utxo_id = UtxoId {
            tx_hash: H256::zero(),
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 500,
            owner: address,
            script_hash: None,
        };
        batch.put(
            CF_UTXOS,
            bincode::serialize(&utxo_id).unwrap(),
            bincode::serialize(&utxo).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // Build a tx that lists the same UTxO input TWICE, trying to claim 1000
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id.clone(), utxo_id.clone()],
            outputs: vec![aether_types::UtxoOutput {
                amount: 900,
                owner: PublicKey::from_bytes(keypair.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let result = ledger.apply_transaction(&tx);
        assert!(result.is_err(), "duplicate UTxO input must be rejected");
        assert!(
            result.unwrap_err().to_string().contains("duplicate UTxO input"),
            "error should mention duplicate input"
        );
    }

    /// Same duplicate-input check must work in the speculative (overlay) path.
    #[test]
    fn test_duplicate_utxo_input_rejected_speculative() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Seed account
        let account = Account::with_balance(address, 0);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );

        // Create a UTxO worth 500
        let utxo_id = UtxoId {
            tx_hash: H256::zero(),
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 500,
            owner: address,
            script_hash: None,
        };
        batch.put(
            CF_UTXOS,
            bincode::serialize(&utxo_id).unwrap(),
            bincode::serialize(&utxo).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // Build tx with duplicate input
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id.clone(), utxo_id.clone()],
            outputs: vec![aether_types::UtxoOutput {
                amount: 900,
                owner: PublicKey::from_bytes(keypair.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let (receipts, _overlay) = ledger.apply_block_speculatively(&[tx]).unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(
            matches!(&receipts[0].status, TransactionStatus::Failed { reason } if reason.contains("duplicate UTxO input")),
            "speculative path must reject duplicate UTxO inputs, got: {:?}",
            receipts[0].status
        );
    }

    /// Cross-tx double-spend within a single block: tx1 spends a UTxO,
    /// tx2 tries to spend the same UTxO — tx2 must fail.
    #[test]
    fn test_cross_tx_double_spend_in_block_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let keypair = Keypair::generate();
        let address = Address::from_slice(&keypair.to_address()).unwrap();

        // Seed account
        let account = Account::with_balance(address, 0);
        let mut batch = StorageBatch::new();
        batch.put(
            CF_ACCOUNTS,
            address.as_bytes().to_vec(),
            bincode::serialize(&account).unwrap(),
        );

        // Create a single UTxO worth 1000
        let utxo_id = UtxoId {
            tx_hash: H256::zero(),
            output_index: 0,
        };
        let utxo = Utxo {
            amount: 1000,
            owner: address,
            script_hash: None,
        };
        batch.put(
            CF_UTXOS,
            bincode::serialize(&utxo_id).unwrap(),
            bincode::serialize(&utxo).unwrap(),
        );
        ledger.storage.write_batch(batch).unwrap();

        // tx1: spends the UTxO legitimately
        let mut tx1 = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id.clone()],
            outputs: vec![aether_types::UtxoOutput {
                amount: 800,
                owner: PublicKey::from_bytes(keypair.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash1 = tx1.hash();
        tx1.signature = Signature::from_bytes(keypair.sign(hash1.as_bytes()));

        // tx2: tries to spend the same UTxO again
        let mut tx2 = Transaction {
            nonce: 1,
            chain_id: 1,
            sender: address,
            sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
            inputs: vec![utxo_id.clone()],
            outputs: vec![aether_types::UtxoOutput {
                amount: 800,
                owner: PublicKey::from_bytes(keypair.public_key()),
                script_hash: None,
            }],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21_000,
            fee: 100,
            signature: Signature::from_bytes(vec![]),
        };
        let hash2 = tx2.hash();
        tx2.signature = Signature::from_bytes(keypair.sign(hash2.as_bytes()));

        let (receipts, _overlay) =
            ledger.apply_block_speculatively(&[tx1, tx2]).unwrap();
        assert_eq!(receipts.len(), 2);
        assert!(
            matches!(&receipts[0].status, TransactionStatus::Success),
            "tx1 should succeed"
        );
        assert!(
            matches!(&receipts[1].status, TransactionStatus::Failed { reason } if reason.contains("already spent")),
            "tx2 must fail as double-spend, got: {:?}",
            receipts[1].status
        );
    }

    #[test]
    fn test_fold_fee_distribution_credits_proposer() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let proposer_kp = Keypair::generate();
        let proposer = Address::from_slice(&proposer_kp.to_address()).unwrap();

        // Proposer starts with 0 balance.
        let overlay = PendingOverlay::new();
        let mut batch = StorageBatch::new();
        let reward = 5_000u128;
        ledger
            .fold_fee_distribution_into_batch(&mut batch, &overlay, &proposer, reward, 0)
            .unwrap();
        ledger.write_batch(batch).unwrap();

        let account = ledger.get_or_create_account(&proposer).unwrap();
        assert_eq!(account.balance, reward, "proposer should receive reward");
    }

    #[test]
    fn test_fold_fee_distribution_records_burned() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let proposer_kp = Keypair::generate();
        let proposer = Address::from_slice(&proposer_kp.to_address()).unwrap();

        let overlay = PendingOverlay::new();
        let mut batch = StorageBatch::new();
        ledger
            .fold_fee_distribution_into_batch(&mut batch, &overlay, &proposer, 0, 1_000)
            .unwrap();
        ledger.write_batch(batch).unwrap();

        assert_eq!(ledger.total_burned(), 1_000);
    }

    #[test]
    fn test_fold_fee_distribution_state_root_includes_proposer_credit() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let proposer_kp = Keypair::generate();
        let proposer = Address::from_slice(&proposer_kp.to_address()).unwrap();

        let root_before = ledger.state_root();

        let overlay = PendingOverlay::new();
        let mut batch = StorageBatch::new();
        ledger
            .fold_fee_distribution_into_batch(&mut batch, &overlay, &proposer, 9_000, 0)
            .unwrap();
        ledger.write_batch(batch).unwrap();

        let root_after = ledger.state_root();
        assert_ne!(root_before, root_after, "state root must change after credit");
    }

    #[test]
    fn test_fold_fee_distribution_uses_overlay_for_proposer_balance() {
        // If the proposer has an updated balance in the overlay (e.g. they sent
        // a transaction in this block), fold_fee_distribution_into_batch must use
        // that overlay balance as the base for the credit, not the DB balance.
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();

        let proposer_kp = Keypair::generate();
        let proposer = Address::from_slice(&proposer_kp.to_address()).unwrap();

        // Seed proposer with 1000 in the DB.
        ledger.seed_account(&proposer, 1_000).unwrap();

        // Build an overlay that reduces proposer balance to 800 (simulates a tx fee).
        let reduced_account = Account::with_balance(proposer, 800);
        let serialized = bincode::serialize(&reduced_account).unwrap();
        let mut overlay = PendingOverlay::new();
        overlay.put(CF_ACCOUNTS, proposer.as_bytes().to_vec(), serialized);
        overlay.changed_accounts.push(proposer);

        let mut batch = ledger.prepare_overlay_batch(&overlay).unwrap();
        ledger
            .fold_fee_distribution_into_batch(&mut batch, &overlay, &proposer, 200, 0)
            .unwrap();
        ledger.write_batch(batch).unwrap();

        // Expected: overlay balance (800) + reward (200) = 1000
        let account = ledger.get_or_create_account(&proposer).unwrap();
        assert_eq!(
            account.balance, 1_000,
            "proposer balance should be overlay base (800) + reward (200)"
        );
    }
}
