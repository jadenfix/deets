use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::evolution::verify_auth_path;

/// A KES signature containing the Ed25519 signature from the active leaf,
/// the leaf's public key, and the Merkle authentication path to the root.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KesSignature {
    /// Time period this signature was produced at.
    pub period: u32,
    /// Ed25519 signature bytes (64 bytes as Vec for serde compatibility).
    pub signature: Vec<u8>,
    /// Public key of the active leaf that produced this signature (32 bytes).
    pub leaf_pubkey: [u8; 32],
    /// Merkle authentication path from leaf to root (depth * 32 bytes).
    pub auth_path: Vec<[u8; 32]>,
}

impl KesSignature {
    /// Verify the signature against a KES verification key and message.
    ///
    /// Steps:
    /// 1. Verify the Ed25519 signature against the leaf public key
    /// 2. Verify the Merkle authentication path from leaf to root
    /// 3. Compare the reconstructed root to the verification key's root
    #[must_use = "verification result must not be silently discarded"]
    pub fn verify(&self, vk: &KesVerificationKey, message: &[u8]) -> bool {
        if self.period >= vk.max_periods {
            return false;
        }

        // Validate auth path length matches expected tree depth
        let expected_depth = (vk.max_periods as f64).log2().ceil() as usize;
        if self.auth_path.len() != expected_depth {
            return false;
        }

        // Step 1: Verify Ed25519 signature
        let verifying_key = match VerifyingKey::from_bytes(&self.leaf_pubkey) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let sig_bytes: [u8; 64] = match self.signature.as_slice().try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let signature = Signature::from_bytes(&sig_bytes);
        if verifying_key.verify(message, &signature).is_err() {
            return false;
        }

        // Step 2: Verify Merkle authentication path
        verify_auth_path(
            &self.leaf_pubkey,
            self.period as usize,
            &self.auth_path,
            &vk.root,
        )
    }
}

/// The public verification key for a KES scheme.
/// Contains the Merkle root of all leaf public keys.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KesVerificationKey {
    pub(crate) root: [u8; 32],
    pub(crate) max_periods: u32,
}

impl KesVerificationKey {
    pub fn new(root: [u8; 32], max_periods: u32) -> Self {
        KesVerificationKey { root, max_periods }
    }

    pub fn root(&self) -> [u8; 32] {
        self.root
    }

    pub fn max_periods(&self) -> u32 {
        self.max_periods
    }
}

#[cfg(test)]
mod tests {
    use crate::evolution::KesKey;

    #[test]
    fn test_kes_verify_roundtrip() {
        let mut key = KesKey::generate(8);
        let vk = key.verification_key();
        let sig = key.sign(3, b"msg").unwrap();

        assert!(sig.verify(&vk, b"msg"));
        assert!(!sig.verify(&vk, b"other"));
    }

    #[test]
    fn test_kes_signature_at_different_periods() {
        let mut key = KesKey::generate(8);
        let vk = key.verification_key();

        let sig0 = key.sign(0, b"zero").unwrap();
        let sig3 = key.sign(3, b"three").unwrap();
        let sig7 = key.sign(7, b"seven").unwrap();

        assert!(sig0.verify(&vk, b"zero"));
        assert!(sig3.verify(&vk, b"three"));
        assert!(sig7.verify(&vk, b"seven"));

        // Cross-verification should fail
        assert!(!sig0.verify(&vk, b"three"));
        assert!(!sig3.verify(&vk, b"zero"));
    }

    #[test]
    fn test_oversized_auth_path_rejected() {
        let mut key = KesKey::generate(8); // depth=3 (8 periods -> 2^3)
        let vk = key.verification_key();
        let message = b"test message";

        // Get a valid signature first
        let sig = key.sign(0, message).unwrap();
        assert!(sig.verify(&vk, message), "Valid signature should verify");

        // Create a tampered signature with extra auth_path elements
        let mut tampered = sig.clone();
        tampered.auth_path.push([0xAA; 32]); // Add extra element
        assert!(
            !tampered.verify(&vk, message),
            "Oversized auth_path must be rejected"
        );
    }

    #[test]
    fn test_undersized_auth_path_rejected() {
        let mut key = KesKey::generate(8); // depth=3
        let vk = key.verification_key();
        let message = b"test message";

        let sig = key.sign(0, message).unwrap();

        // Create a tampered signature with missing auth_path elements
        let mut tampered = sig.clone();
        tampered.auth_path.pop(); // Remove last element
        assert!(
            !tampered.verify(&vk, message),
            "Undersized auth_path must be rejected"
        );
    }
}
