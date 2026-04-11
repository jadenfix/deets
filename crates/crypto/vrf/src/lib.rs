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
        #[inline]
        #[must_use]
        pub fn new(public_key: [u8; 32]) -> Self {
            Self { public_key }
        }

        #[inline]
        #[must_use]
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

#[cfg(test)]
mod prop_tests {
    use super::mock::*;
    use super::*;
    use proptest::prelude::*;

    fn arb_public_key() -> impl Strategy<Value = [u8; 32]> {
        prop::array::uniform32(any::<u8>())
    }

    proptest! {
        #[test]
        fn mock_signer_deterministic_for_arbitrary_inputs(
            pk in arb_public_key(),
            alpha in prop::collection::vec(any::<u8>(), 0..256),
        ) {
            let signer = MockVrfSigner::new(pk);
            let p1 = signer.prove(&alpha);
            let p2 = signer.prove(&alpha);
            prop_assert_eq!(p1.output, p2.output);
            prop_assert_eq!(p1.proof, p2.proof);
        }

        #[test]
        fn mock_signer_public_key_roundtrip(pk in arb_public_key()) {
            let signer = MockVrfSigner::new(pk);
            prop_assert_eq!(signer.public_key_bytes(), pk);
        }

        #[test]
        fn mock_signer_different_keys_produce_different_outputs(
            idx_a in 0u8..128,
            idx_b in 128u8..=255,
            alpha in prop::collection::vec(any::<u8>(), 1..64),
        ) {
            let a = MockVrfSigner::from_index(idx_a);
            let b = MockVrfSigner::from_index(idx_b);
            let pa = a.prove(&alpha);
            let pb = b.prove(&alpha);
            prop_assert_ne!(pa.output, pb.output);
        }

        #[test]
        fn mock_signer_proof_is_80_bytes(
            pk in arb_public_key(),
            alpha in prop::collection::vec(any::<u8>(), 0..128),
        ) {
            let signer = MockVrfSigner::new(pk);
            let proof = signer.prove(&alpha);
            prop_assert_eq!(proof.proof.len(), 80);
        }

        #[test]
        fn ecvrf_verifier_rejects_wrong_key(
            alpha in prop::collection::vec(any::<u8>(), 1..128),
        ) {
            let real_keypair = VrfKeypair::generate();
            let wrong_keypair = VrfKeypair::generate();
            let proof = real_keypair.prove(&alpha);
            let verifier = EcVrfVerifier;
            let result = verifier.verify(wrong_keypair.public_key(), &alpha, &proof);
            if let Ok(valid) = result {
                prop_assert!(!valid);
            }
        }

        #[test]
        fn ecvrf_verifier_rejects_wrong_alpha(
            alpha in prop::collection::vec(any::<u8>(), 1..64),
            extra_byte in any::<u8>(),
        ) {
            let keypair = VrfKeypair::generate();
            let proof = keypair.prove(&alpha);
            let mut wrong_alpha = alpha.clone();
            wrong_alpha.push(extra_byte);
            let verifier = EcVrfVerifier;
            let result = verifier.verify(keypair.public_key(), &wrong_alpha, &proof);
            if let Ok(valid) = result {
                prop_assert!(!valid);
            }
        }

        #[test]
        fn ecvrf_sign_then_verify_roundtrip(
            alpha in prop::collection::vec(any::<u8>(), 0..256),
        ) {
            let keypair = VrfKeypair::generate();
            let signer: &dyn VrfSigner = &keypair;
            let verifier: &dyn VrfVerifier = &EcVrfVerifier;
            let proof = signer.prove(&alpha);
            let valid = verifier.verify(&signer.public_key_bytes(), &alpha, &proof).unwrap();
            prop_assert!(valid);
        }

        #[test]
        fn boxed_signer_matches_direct_signer(
            alpha in prop::collection::vec(any::<u8>(), 0..128),
        ) {
            let pk = [42u8; 32];
            let direct = MockVrfSigner::new(pk);
            let boxed: Box<dyn VrfSigner> = Box::new(MockVrfSigner::new(pk));

            let p_direct = direct.prove(&alpha);
            let p_boxed = boxed.prove(&alpha);

            prop_assert_eq!(p_direct.output, p_boxed.output);
            prop_assert_eq!(p_direct.proof, p_boxed.proof);
            prop_assert_eq!(direct.public_key_bytes(), boxed.public_key_bytes());
        }

        #[test]
        fn boxed_verifier_matches_direct_verifier(
            alpha in prop::collection::vec(any::<u8>(), 0..128),
        ) {
            let keypair = VrfKeypair::generate();
            let proof = keypair.prove(&alpha);

            let direct = EcVrfVerifier;
            let boxed: Box<dyn VrfVerifier> = Box::new(EcVrfVerifier);

            let r_direct = direct.verify(keypair.public_key(), &alpha, &proof).unwrap();
            let r_boxed = boxed.verify(keypair.public_key(), &alpha, &proof).unwrap();
            prop_assert_eq!(r_direct, r_boxed);
        }

        #[test]
        fn mock_verifier_accepts_garbage_proofs(
            pk in arb_public_key(),
            alpha in prop::collection::vec(any::<u8>(), 0..64),
            garbage_proof in prop::collection::vec(any::<u8>(), 0..100),
            garbage_output in arb_public_key(),
        ) {
            let verifier = MockVrfVerifier;
            let proof = VrfProof { proof: garbage_proof, output: garbage_output };
            prop_assert!(verifier.verify(&pk, &alpha, &proof).unwrap());
        }

        #[test]
        fn reject_all_verifier_rejects_valid_proofs(
            alpha in prop::collection::vec(any::<u8>(), 0..64),
        ) {
            let keypair = VrfKeypair::generate();
            let proof = keypair.prove(&alpha);
            let verifier = RejectAllVrfVerifier;
            prop_assert!(!verifier.verify(keypair.public_key(), &alpha, &proof).unwrap());
        }
    }
}
