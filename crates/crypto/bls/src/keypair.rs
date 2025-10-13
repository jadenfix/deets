use anyhow::{anyhow, Result};
use blst::min_pk::{
    PublicKey as BlstPublicKey, SecretKey as BlstSecretKey, Signature as BlstSignature,
};
use blst::BLST_ERROR;

const DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

/// BLS12-381 Keypair for signing and verification
///
/// Uses BLS12-381 curve for:
/// - Signature aggregation (combine 1000s of signatures into one)
/// - Public key aggregation
/// - Single pairing verification for all aggregated signatures
///
/// Key sizes:
/// - Secret key: 32 bytes
/// - Public key: 48 bytes (G1 point compressed)
/// - Signature: 96 bytes (G2 point compressed)

#[derive(Clone)]
pub struct BlsKeypair {
    secret: BlstSecretKey,
    public: BlstPublicKey,
}

impl BlsKeypair {
    /// Generate a new random BLS keypair
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut rng = rand::thread_rng();

        // Generate 32-byte secret key
        let mut ikm = [0u8; 32];
        rng.fill_bytes(&mut ikm);

        let secret =
            BlstSecretKey::key_gen(&ikm, &[]).expect("random IKM always valid for key generation");
        let public = secret.sk_to_pk();

        BlsKeypair { secret, public }
    }

    /// Create keypair from secret key
    pub fn from_secret(secret: Vec<u8>) -> Result<Self> {
        if secret.len() != 32 {
            anyhow::bail!("BLS secret key must be 32 bytes");
        }

        let secret = BlstSecretKey::from_bytes(&secret)
            .map_err(|e| anyhow!("invalid secret key bytes: {:?}", e))?;
        let public = secret.sk_to_pk();

        Ok(BlsKeypair { secret, public })
    }

    /// Sign a message
    /// Returns 96-byte BLS signature
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.secret.sign(message, DST, &[]).to_bytes().to_vec()
    }

    /// Get public key (compressed 48-byte form)
    pub fn public_key(&self) -> Vec<u8> {
        self.public.to_bytes().to_vec()
    }

    /// Get secret key bytes (32-byte scalar)
    pub fn secret_key(&self) -> Vec<u8> {
        self.secret.to_bytes().to_vec()
    }
}

/// Verify a single BLS signature
pub fn verify(public_key: &[u8], _message: &[u8], signature: &[u8]) -> Result<bool> {
    if public_key.len() != 48 {
        anyhow::bail!("BLS public key must be 48 bytes");
    }

    if signature.len() != 96 {
        anyhow::bail!("BLS signature must be 96 bytes");
    }

    let pk = BlstPublicKey::from_bytes(public_key)
        .map_err(|e| anyhow!("invalid public key: {:?}", e))?;
    let sig = BlstSignature::from_bytes(signature)
        .map_err(|e| anyhow!("invalid signature bytes: {:?}", e))?;

    Ok(sig.verify(true, _message, DST, &[], &pk, true) == BLST_ERROR::BLST_SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = BlsKeypair::generate();

        assert_eq!(keypair.secret_key().len(), 32);
        assert_eq!(keypair.public_key().len(), 48);
    }

    #[test]
    fn test_signing() {
        let keypair = BlsKeypair::generate();
        let message = b"test message";

        let signature = keypair.sign(message);

        assert_eq!(signature.len(), 96);
    }

    #[test]
    fn test_deterministic_signing() {
        let keypair = BlsKeypair::generate();
        let message = b"test message";

        let sig1 = keypair.sign(message);
        let sig2 = keypair.sign(message);

        // Same message should produce same signature
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_different_messages() {
        let keypair = BlsKeypair::generate();

        let sig1 = keypair.sign(b"message1");
        let sig2 = keypair.sign(b"message2");

        // Different messages should produce different signatures
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_verification() {
        let keypair = BlsKeypair::generate();
        let message = b"test message";
        let signature = keypair.sign(message);

        let verified = verify(&keypair.public_key(), message, &signature).unwrap();
        assert!(verified);
    }
}
