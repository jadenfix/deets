use std::collections::HashSet;

use aether_crypto_primitives::Keypair;
use aether_types::{Address, PublicKey, Signature, Transaction};
use anyhow::{anyhow, bail, Result};

use crate::types::{ClientConfig, TransferRequest};

pub struct TransferBuilder<'a> {
    config: &'a ClientConfig,
    recipient: Option<Address>,
    amount: Option<u128>,
    memo: Option<String>,
    fee: u128,
    gas_limit: u64,
}

impl<'a> TransferBuilder<'a> {
    pub(crate) fn new(config: &'a ClientConfig) -> Self {
        TransferBuilder {
            config,
            recipient: None,
            amount: None,
            memo: None,
            fee: config.default_fee,
            gas_limit: config.default_gas_limit,
        }
    }

    pub fn to(mut self, recipient: Address) -> Self {
        self.recipient = Some(recipient);
        self
    }

    pub fn amount(mut self, amount: u128) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn memo<T: Into<String>>(mut self, memo: T) -> Self {
        self.memo = Some(memo.into());
        self
    }

    pub fn fee(mut self, fee: u128) -> Self {
        self.fee = fee;
        self
    }

    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

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
        tx.calculate_fee()?;
        Ok(tx)
    }
}
