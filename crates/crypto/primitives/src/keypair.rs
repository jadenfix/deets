use crate::ed25519;

pub struct Keypair {
    inner: ed25519::Keypair,
}

impl Keypair {
    pub fn generate() -> Self {
        Keypair {
            inner: ed25519::Keypair::generate(),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ed25519::Ed25519Error> {
        Ok(Keypair {
            inner: ed25519::Keypair::from_bytes(bytes)?,
        })
    }

    pub fn public_key(&self) -> Vec<u8> {
        self.inner.public_key()
    }

    pub fn secret_key(&self) -> Vec<u8> {
        self.inner.secret_key()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.inner.sign(message)
    }

    pub fn to_address(&self) -> [u8; 20] {
        use sha2::{Digest, Sha256};
        let pubkey = self.public_key();
        let hash = Sha256::digest(&pubkey);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[..20]);
        addr
    }
}
