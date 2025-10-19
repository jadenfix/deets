use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct H256(pub [u8; 32]);

impl H256 {
    pub const fn zero() -> Self {
        H256([0u8; 32])
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != 32 {
            return Err("invalid length");
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(H256(arr))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for H256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl fmt::Display for H256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0[..8]))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct H160(pub [u8; 20]);

impl H160 {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != 20 {
            return Err("invalid length");
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(bytes);
        Ok(H160(arr))
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

impl fmt::Debug for H160 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl fmt::Display for H160 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

pub type Address = H160;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(pub Vec<u8>);

impl Signature {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Signature(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Sig({})", hex::encode(&self.0))
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey(pub Vec<u8>);

impl PublicKey {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        PublicKey(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_address(&self) -> Address {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&self.0);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[..20]);
        H160(addr)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PubKey({})", hex::encode(&self.0))
    }
}

pub type Slot = u64;
pub type Epoch = u64;

pub fn slot_to_epoch(slot: Slot, epoch_slots: u64) -> Epoch {
    slot / epoch_slots
}

pub fn epoch_start_slot(epoch: Epoch, epoch_slots: u64) -> Slot {
    epoch * epoch_slots
}
