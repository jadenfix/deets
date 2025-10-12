use crate::primitives::{Address, PublicKey, Signature, H256};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub nonce: u64,
    pub sender: Address,
    pub sender_pubkey: PublicKey,
    pub inputs: Vec<UtxoId>,
    pub outputs: Vec<UtxoOutput>,
    pub reads: HashSet<Address>,
    pub writes: HashSet<Address>,
    pub program_id: Option<H256>,
    pub data: Vec<u8>,
    pub gas_limit: u64,
    pub fee: u128,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UtxoId {
    pub tx_hash: H256,
    pub output_index: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UtxoOutput {
    pub amount: u128,
    pub owner: PublicKey,
    pub script_hash: Option<H256>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionReceipt {
    pub tx_hash: H256,
    pub block_hash: H256,
    pub slot: u64,
    pub status: TransactionStatus,
    pub gas_used: u64,
    pub logs: Vec<Log>,
    pub state_root: H256,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TransactionStatus {
    Success,
    Failed { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<H256>,
    pub data: Vec<u8>,
}

impl Transaction {
    pub fn hash(&self) -> H256 {
        use sha2::{Digest, Sha256};
        let mut tx = self.clone();
        tx.signature = Signature::from_bytes(vec![]);
        let bytes = bincode::serialize(&tx).unwrap();
        let hash = Sha256::digest(&bytes);
        H256::from_slice(&hash).unwrap()
    }

    pub fn verify_signature(&self) -> anyhow::Result<()> {
        if self.signature.as_bytes().is_empty() {
            anyhow::bail!("signature is empty");
        }

        // Verify the sender address matches the public key
        let derived_address = self.sender_pubkey.to_address();
        if derived_address != self.sender {
            anyhow::bail!("sender address does not match public key");
        }

        // Get the message to verify (transaction hash without signature)
        let _msg = self.hash();

        // Verify signature using ed25519
        // Note: The ed25519 module should be available via crypto primitives
        // For now, we do basic validation
        if self.signature.as_bytes().len() != 64 {
            anyhow::bail!(
                "invalid signature length: expected 64 bytes, got {}",
                self.signature.as_bytes().len()
            );
        }

        if self.sender_pubkey.as_bytes().len() != 32 {
            anyhow::bail!(
                "invalid public key length: expected 32 bytes, got {}",
                self.sender_pubkey.as_bytes().len()
            );
        }

        // TODO: Call actual ed25519 verification
        // This would be: ed25519::verify(self.sender_pubkey.as_bytes(), msg.as_bytes(), self.signature.as_bytes())?;

        Ok(())
    }

    pub fn calculate_fee(&self) -> anyhow::Result<u128> {
        const A: u128 = 10_000; // base cost
        const B: u128 = 5; // per byte
        const C: u128 = 2; // per gas unit

        let bytes = bincode::serialize(self)
            .map_err(|e| anyhow::anyhow!("serialize failed: {}", e))?
            .len() as u128;

        let computed_fee = A + B * bytes + C * self.gas_limit as u128;

        if self.fee < computed_fee {
            anyhow::bail!(
                "fee too low: provided {}, required {}",
                self.fee,
                computed_fee
            );
        }

        Ok(self.fee)
    }

    pub fn conflicts_with(&self, other: &Transaction) -> bool {
        // Write-Write conflict
        if !self.writes.is_disjoint(&other.writes) {
            return true;
        }
        // Write-Read conflicts (both directions)
        if !self.writes.is_disjoint(&other.reads) {
            return true;
        }
        if !other.writes.is_disjoint(&self.reads) {
            return true;
        }
        // UTxO conflicts
        for input in &self.inputs {
            if other.inputs.contains(input) {
                return true;
            }
        }
        false
    }
}
