use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey, Verifier, VerifyingKey};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Ed25519Error {
    #[error("invalid signature")]
    Signature,
    #[error("invalid public key")]
    PublicKey,
    #[error("invalid secret key")]
    SecretKey,
}

pub struct Keypair {
    signing_key: SigningKey,
}

impl Keypair {
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        Keypair { signing_key }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Ed25519Error> {
        if bytes.len() != 32 {
            return Err(Ed25519Error::SecretKey);
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        Ok(Keypair { signing_key })
    }

    pub fn public_key(&self) -> Vec<u8> {
        self.signing_key.verifying_key().to_bytes().to_vec()
    }

    pub fn secret_key(&self) -> Vec<u8> {
        self.signing_key.to_bytes().to_vec()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.signing_key.sign(message).to_bytes().to_vec()
    }
}

pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), Ed25519Error> {
    if public_key.len() != 32 {
        return Err(Ed25519Error::PublicKey);
    }
    if signature.len() != 64 {
        return Err(Ed25519Error::Signature);
    }

    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(public_key);
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| Ed25519Error::PublicKey)?;

    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(signature);
    let signature = DalekSignature::from_bytes(&sig_bytes);

    verifying_key
        .verify(message, &signature)
        .map_err(|_| Ed25519Error::Signature)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_verify() {
        let keypair = Keypair::generate();
        let message = b"hello world";
        let signature = keypair.sign(message);
        let public_key = keypair.public_key();

        assert!(verify(&public_key, message, &signature).is_ok());
    }

    #[test]
    fn test_invalid_signature() {
        let keypair = Keypair::generate();
        let message = b"hello world";
        let mut signature = keypair.sign(message);
        signature[0] ^= 0x01; // Corrupt signature
        let public_key = keypair.public_key();

        assert!(verify(&public_key, message, &signature).is_err());
    }
}
