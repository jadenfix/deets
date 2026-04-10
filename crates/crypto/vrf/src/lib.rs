pub mod ecvrf;

pub use ecvrf::{check_leader_eligibility_integer, verify_proof, VrfKeypair, VrfProof};

#[allow(deprecated)]
pub use ecvrf::{check_leader_eligibility, output_to_value};

use anyhow::Result;

/// VRF signing: produces proofs binding a pseudorandom output to a secret key
/// and an input message. Extracted as a trait so consensus can be tested with
/// deterministic mock implementations.
pub trait VrfSigner: Send + Sync {
    fn public_key_bytes(&self) -> [u8; 32];
    fn prove(&self, alpha: &[u8]) -> VrfProof;
}

/// VRF verification: validates that a proof was correctly produced by the holder
/// of the secret key corresponding to the given public key.
pub trait VrfVerifier: Send + Sync {
    fn verify(&self, public_key: &[u8; 32], alpha: &[u8], proof: &VrfProof) -> Result<bool>;
}

impl VrfSigner for VrfKeypair {
    fn public_key_bytes(&self) -> [u8; 32] {
        *self.public_key()
    }

    fn prove(&self, alpha: &[u8]) -> VrfProof {
        VrfKeypair::prove(self, alpha)
    }
}

/// ECVRF-EDWARDS25519-SHA512-ELL2 verifier (RFC 9381).
pub struct EcVrfVerifier;

impl VrfVerifier for EcVrfVerifier {
    fn verify(&self, public_key: &[u8; 32], alpha: &[u8], proof: &VrfProof) -> Result<bool> {
        verify_proof(public_key, alpha, proof)
    }
}

pub mod mock {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Deterministic mock VRF for testing. Output = SHA-256(public_key || alpha).
    /// All proofs verify. Useful for writing consensus tests with predictable
    /// leader election outcomes.
    #[derive(Clone, Debug)]
    pub struct MockVrfSigner {
        public_key: [u8; 32],
    }

    impl MockVrfSigner {
        pub fn new(public_key: [u8; 32]) -> Self {
            Self { public_key }
        }

        pub fn from_index(index: u8) -> Self {
            let mut pk = [0u8; 32];
            pk[0] = index;
            Self { public_key: pk }
        }
    }

    impl VrfSigner for MockVrfSigner {
        fn public_key_bytes(&self) -> [u8; 32] {
            self.public_key
        }

        fn prove(&self, alpha: &[u8]) -> VrfProof {
            let mut hasher = Sha256::new();
            hasher.update(self.public_key);
            hasher.update(alpha);
            let hash = hasher.finalize();
            let mut output = [0u8; 32];
            output.copy_from_slice(&hash);

            VrfProof {
                proof: vec![0u8; 80],
                output,
            }
        }
    }

    /// Mock verifier that accepts all proofs (for testing consensus logic
    /// without real cryptographic verification).
    pub struct MockVrfVerifier;

    impl VrfVerifier for MockVrfVerifier {
        fn verify(&self, _public_key: &[u8; 32], _alpha: &[u8], _proof: &VrfProof) -> Result<bool> {
            Ok(true)
        }
    }

    /// Mock verifier that rejects all proofs.
    pub struct RejectAllVrfVerifier;

    impl VrfVerifier for RejectAllVrfVerifier {
        fn verify(&self, _public_key: &[u8; 32], _alpha: &[u8], _proof: &VrfProof) -> Result<bool> {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod trait_tests {
    use super::mock::*;
    use super::*;

    #[test]
    fn ecvrf_implements_signer_trait() {
        let keypair = VrfKeypair::generate();
        let signer: &dyn VrfSigner = &keypair;
        let proof = signer.prove(b"test input");
        assert_eq!(proof.proof.len(), 80);
        assert_eq!(signer.public_key_bytes(), *keypair.public_key());
    }

    #[test]
    fn ecvrf_verifier_works_with_trait() {
        let keypair = VrfKeypair::generate();
        let signer: &dyn VrfSigner = &keypair;
        let verifier: &dyn VrfVerifier = &EcVrfVerifier;

        let proof = signer.prove(b"hello");
        let valid = verifier
            .verify(&signer.public_key_bytes(), b"hello", &proof)
            .unwrap();
        assert!(valid);

        let invalid = verifier
            .verify(&signer.public_key_bytes(), b"wrong", &proof)
            .unwrap();
        assert!(!invalid);
    }

    #[test]
    fn mock_signer_is_deterministic() {
        let signer = MockVrfSigner::from_index(1);
        let p1 = signer.prove(b"slot-42");
        let p2 = signer.prove(b"slot-42");
        assert_eq!(p1.output, p2.output);
    }

    #[test]
    fn mock_signer_different_inputs_differ() {
        let signer = MockVrfSigner::from_index(1);
        let p1 = signer.prove(b"slot-1");
        let p2 = signer.prove(b"slot-2");
        assert_ne!(p1.output, p2.output);
    }

    #[test]
    fn mock_verifier_accepts_all() {
        let verifier = MockVrfVerifier;
        let proof = VrfProof {
            proof: vec![0u8; 80],
            output: [0u8; 32],
        };
        assert!(verifier.verify(&[0u8; 32], b"any", &proof).unwrap());
    }

    #[test]
    fn reject_all_verifier_rejects_all() {
        let verifier = RejectAllVrfVerifier;
        let keypair = VrfKeypair::generate();
        let proof = keypair.prove(b"test");
        assert!(!verifier
            .verify(keypair.public_key(), b"test", &proof)
            .unwrap());
    }

    #[test]
    fn trait_objects_are_object_safe() {
        let keypair = VrfKeypair::generate();
        let signer: Box<dyn VrfSigner> = Box::new(keypair);
        let verifier: Box<dyn VrfVerifier> = Box::new(EcVrfVerifier);

        let proof = signer.prove(b"object safety");
        let valid = verifier
            .verify(&signer.public_key_bytes(), b"object safety", &proof)
            .unwrap();
        assert!(valid);
    }

    #[test]
    fn mock_can_replace_real_vrf_in_box() {
        let signer: Box<dyn VrfSigner> = Box::new(MockVrfSigner::from_index(5));
        let verifier: Box<dyn VrfVerifier> = Box::new(MockVrfVerifier);

        let proof = signer.prove(b"mock-slot");
        assert!(verifier
            .verify(&signer.public_key_bytes(), b"mock-slot", &proof)
            .unwrap());
    }
}
