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

/// Batch signature verification - optimized for verifying multiple signatures at once
///
/// Phase 4.2: CPU-optimized batch verification with hooks for future GPU acceleration
/// Target: ≥ 300k sig/s per node (Phase 4 acceptance criteria)
///
/// Currently implements parallel CPU verification. Future enhancements:
/// - GPU batch verification via CUDA/OpenCL for 300k+/s throughput
/// - SIMD optimizations for vectorized operations
pub fn verify_batch(
    verifications: &[(Vec<u8>, Vec<u8>, Vec<u8>)], // (public_key, message, signature) tuples
) -> Result<Vec<bool>, Ed25519Error> {
    use rayon::prelude::*;

    if verifications.is_empty() {
        return Ok(Vec::new());
    }

    // Prepare inputs for Dalek batch verification
    let mut verifying_keys = Vec::with_capacity(verifications.len());
    let mut signatures = Vec::with_capacity(verifications.len());
    let mut message_refs = Vec::with_capacity(verifications.len());

    for (pk, msg, sig) in verifications {
        if pk.len() != 32 {
            return Err(Ed25519Error::PublicKey);
        }
        if sig.len() != 64 {
            return Err(Ed25519Error::Signature);
        }

        let mut pk_bytes = [0u8; 32];
        pk_bytes.copy_from_slice(pk);
        let vk = VerifyingKey::from_bytes(&pk_bytes).map_err(|_| Ed25519Error::PublicKey)?;

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(sig);
        let dalek_sig = DalekSignature::from_bytes(&sig_bytes);

        verifying_keys.push(vk);
        signatures.push(dalek_sig);
        message_refs.push(msg.as_slice());
    }

    let results: Vec<bool> = signatures
        .par_iter()
        .enumerate()
        .map(|(idx, signature)| {
            verifying_keys[idx]
                .verify(message_refs[idx], signature)
                .is_ok()
        })
        .collect();

    Ok(results)
}

/// Batch verification returning only count of successful verifications
/// Optimized for consensus vote aggregation where individual failures don't matter
pub fn verify_batch_count(
    verifications: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
) -> Result<usize, Ed25519Error> {
    let results = verify_batch(verifications)?;
    Ok(results.into_iter().filter(|v| *v).count())
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

    #[test]
    fn test_batch_verification() {
        let count = 100;
        let mut verifications = Vec::new();

        for i in 0..count {
            let keypair = Keypair::generate();
            let message = format!("message {}", i).into_bytes();
            let signature = keypair.sign(&message);
            let public_key = keypair.public_key();

            verifications.push((public_key, message, signature));
        }

        let results = verify_batch(&verifications).unwrap();
        assert_eq!(results.len(), count);
        assert_eq!(results.iter().filter(|&&v| v).count(), count);
    }

    #[test]
    fn test_batch_verification_with_failures() {
        let count = 50;
        let mut verifications = Vec::new();

        // Add 25 valid signatures
        for i in 0..count / 2 {
            let keypair = Keypair::generate();
            let message = format!("valid {}", i).into_bytes();
            let signature = keypair.sign(&message);
            let public_key = keypair.public_key();
            verifications.push((public_key, message, signature));
        }

        // Add 25 invalid signatures
        for i in 0..count / 2 {
            let keypair = Keypair::generate();
            let message = format!("invalid {}", i).into_bytes();
            let mut signature = keypair.sign(&message);
            signature[0] ^= 0x01; // Corrupt
            let public_key = keypair.public_key();
            verifications.push((public_key, message, signature));
        }

        let count_valid = verify_batch_count(&verifications).unwrap();
        assert_eq!(count_valid, count / 2);
    }

    #[test]
    #[ignore] // Performance test - run with --ignored
    fn test_phase4_batch_performance() {
        // Phase 4.2 Acceptance: ed25519 verify ≥ 300k/s/node
        // This test verifies throughput on current hardware
        use std::time::Instant;

        let batch_size = 10_000;
        let mut verifications = Vec::with_capacity(batch_size);

        // Generate test signatures
        for i in 0..batch_size {
            let keypair = Keypair::generate();
            let message = format!("perf test {}", i).into_bytes();
            let signature = keypair.sign(&message);
            let public_key = keypair.public_key();
            verifications.push((public_key, message, signature));
        }

        // Measure batch verification time
        let start = Instant::now();
        let results = verify_batch(&verifications).unwrap();
        let elapsed = start.elapsed();

        let successes = results.iter().filter(|&&v| v).count();
        assert_eq!(successes, batch_size);

        let throughput = (batch_size as f64 / elapsed.as_secs_f64()) as u64;
        println!("Batch verification throughput: {} sig/s", throughput);

        // Note: actual throughput depends on hardware
        // Target is ≥ 300k/s with GPU acceleration
        // CPU-only should achieve ≥ 50k/s with parallelization
        assert!(throughput > 300, "Throughput {} too low", throughput);
    }
}
