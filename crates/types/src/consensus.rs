use crate::primitives::{PublicKey, Signature, Slot, H256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vote {
    pub slot: Slot,
    pub block_hash: H256,
    pub validator: PublicKey,
    pub signature: Signature,
    pub stake: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub pubkey: PublicKey,
    pub stake: u128,
    pub commission: u16,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpochInfo {
    pub epoch: u64,
    pub start_slot: Slot,
    pub end_slot: Slot,
    pub randomness: H256,
    pub validators: Vec<ValidatorInfo>,
    pub total_stake: u128,
}
