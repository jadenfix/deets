use crate::primitives::{Address, PublicKey, Slot, H256};
use crate::transaction::Transaction;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub aggregated_vote: Option<AggregatedVote>,
}

/// Current protocol version. Incremented on hard forks.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Protocol version (for hard fork signaling).
    pub version: u32,
    pub slot: Slot,
    pub parent_hash: H256,
    pub state_root: H256,
    pub transactions_root: H256,
    pub receipts_root: H256,
    pub proposer: Address,
    pub vrf_proof: VrfProof,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VrfProof {
    pub output: [u8; 32],
    pub proof: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AggregatedVote {
    pub slot: Slot,
    pub block_hash: H256,
    pub aggregated_signature: Vec<u8>,
    pub signers: Vec<PublicKey>,
    pub total_stake: u128,
}

impl Block {
    pub fn hash(&self) -> H256 {
        use sha2::{Digest, Sha256};
        let bytes = bincode::serialize(&self.header).expect("header serialization infallible");
        let hash = Sha256::digest(&bytes);
        H256::from_slice(&hash).expect("SHA256 produces 32 bytes")
    }

    pub fn new(
        slot: Slot,
        parent_hash: H256,
        proposer: Address,
        vrf_proof: VrfProof,
        transactions: Vec<Transaction>,
    ) -> Self {
        Block {
            header: BlockHeader {
                version: PROTOCOL_VERSION,
                slot,
                parent_hash,
                state_root: H256::zero(),
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                proposer,
                vrf_proof,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
            transactions,
            aggregated_vote: None,
        }
    }
}
