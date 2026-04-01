use std::collections::HashSet;

use aether_crypto_primitives::Keypair;
use aether_types::{Address, PublicKey, Signature, Transaction};
use anyhow::{anyhow, bail, Result};

use crate::types::{ClientConfig, TransferRequest};

/// Builder for constructing token transfer transactions.
pub struct TransferBuilder {
    recipient: Option<Address>,
    amount: Option<u128>,
    memo: Option<String>,
    fee: u128,
    gas_limit: u64,
    chain_id: u64,
}

impl TransferBuilder {
    pub(crate) fn new(config: &ClientConfig) -> Self {
        TransferBuilder {
            recipient: None,
            amount: None,
            memo: None,
            fee: config.default_fee,
            gas_limit: config.default_gas_limit,
            chain_id: 1,
        }
    }

    /// Set the recipient address.
    pub fn to(mut self, recipient: Address) -> Self {
        self.recipient = Some(recipient);
        self
    }

    /// Set the transfer amount in base units.
    pub fn amount(mut self, amount: u128) -> Self {
        self.amount = Some(amount);
        self
    }

    /// Attach an optional memo string to the transfer.
    pub fn memo<T: Into<String>>(mut self, memo: T) -> Self {
        self.memo = Some(memo.into());
        self
    }

    /// Override the default transaction fee.
    pub fn fee(mut self, fee: u128) -> Self {
        self.fee = fee;
        self
    }

    /// Override the default gas limit.
    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    /// Set the chain ID (defaults to 1).
    pub fn chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// Build and sign the transfer transaction.
    pub fn build(self, keypair: &Keypair, nonce: u64) -> Result<Transaction> {
        let recipient = self.recipient.ok_or_else(|| anyhow!("missing recipient"))?;
        let amount = self.amount.ok_or_else(|| anyhow!("missing amount"))?;

        let payload = TransferRequest {
            recipient,
            amount,
            memo: self.memo,
        };

        let payload_bytes = bincode::serialize(&payload)?;
        let sender_pubkey = PublicKey::from_bytes(keypair.public_key());
        let sender_address = sender_pubkey.to_address();

        let mut writes = HashSet::new();
        writes.insert(recipient);

        let mut tx = Transaction {
            nonce,
            chain_id: self.chain_id,
            sender: sender_address,
            sender_pubkey,
            inputs: Vec::new(),
            outputs: Vec::new(),
            reads: HashSet::new(),
            writes,
            program_id: None,
            data: payload_bytes,
            gas_limit: self.gas_limit,
            fee: self.fee,
            signature: Signature::from_bytes(vec![0; 64]),
        };

        let message = tx.hash();
        let signature = keypair.sign(message.as_bytes());
        if signature.len() != 64 {
            bail!("invalid signature length: {}", signature.len());
        }
        tx.signature = Signature::from_bytes(signature);
        tx.verify_signature()?;
        let fee_params = aether_types::ChainConfig::devnet().fees;
        tx.calculate_fee(&fee_params)?;
        Ok(tx)
    }
}
