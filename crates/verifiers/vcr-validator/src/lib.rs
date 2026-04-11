// ============================================================================
// AETHER VCR VALIDATOR - Verifiable Compute Receipt
// ============================================================================
// PURPOSE: Verify AI inference computations are deterministic and correct
//
// VCR COMPONENTS:
// 1. Trace commitment: KZG commitment to execution trace
// 2. TEE attestation: Proof of secure execution
// 3. Input/output hashes: Deterministic I/O
// 4. Metadata: Model hash, timestamp, worker ID
//
// VERIFICATION PROCESS:
// 1. Check TEE attestation (worker ran in TEE)
// 2. Verify KZG commitment (trace matches claimed output)
// 3. Challenge mechanism (spot-check trace validity)
// 4. Worker signature verification
// ============================================================================

use aether_crypto_kzg::{KzgCommitment, KzgProof, KzgVerifier};
use aether_crypto_primitives::ed25519;
use aether_types::H256;
use aether_verifiers_tee::{AttestationReport, TeeVerifier};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableComputeReceipt {
    pub job_id: H256,
    pub worker_id: Vec<u8>,
    pub model_hash: H256,
    pub input_hash: H256,
    pub output_hash: H256,
    pub trace_commitment: Vec<u8>, // KZG commitment (48 bytes)
    #[serde(default)]
    pub trace_proof: Vec<u8>, // KZG opening proof (48 bytes)
    #[serde(default)]
    pub trace_evaluation: Vec<u8>, // Claimed trace evaluation (32 bytes)
    #[serde(default)]
    pub trace_point: Vec<u8>, // Challenge point (32 bytes)
    pub tee_attestation: Vec<u8>,  // JSON-encoded AttestationReport
    pub timestamp: u64,
    pub signature: Vec<u8>, // Ed25519 signature from worker public key
}

pub struct VcrValidator {
    /// Minimum quorum size for consensus
    quorum_size: usize,

    /// Challenge window (slots)
    challenge_window: u64,

    /// TEE attestation verifier
    tee_verifier: TeeVerifier,

    /// KZG verifier for trace checks
    kzg_verifier: KzgVerifier,
}

impl VcrValidator {
    /// Create a VCR validator with explicit configuration.
    /// Use `new_for_test()` for development/testing only.
    pub fn new(
        kzg_verifier: KzgVerifier,
        tee_verifier: TeeVerifier,
        quorum_size: usize,
        challenge_window: u64,
    ) -> Self {
        VcrValidator {
            quorum_size,
            challenge_window,
            tee_verifier,
            kzg_verifier,
        }
    }

    /// Create a VCR validator for development/testing with insecure defaults.
    /// WARNING: Do NOT use in production — uses test KZG parameters and
    /// accepts the default simulation TEE measurement.
    pub fn new_for_test() -> Self {
        let mut tee_verifier = TeeVerifier::new();
        tee_verifier.add_approved_measurement(vec![1u8; 48]);

        VcrValidator {
            quorum_size: 3,
            challenge_window: 10,
            tee_verifier,
            kzg_verifier: KzgVerifier::new_insecure_test(1024),
        }
    }

    pub fn approve_measurement(&mut self, measurement: Vec<u8>) {
        self.tee_verifier.add_approved_measurement(measurement);
    }

    /// Verify a single VCR
    pub fn verify(&self, vcr: &VerifiableComputeReceipt) -> Result<()> {
        // 1. Verify basic fields
        if vcr.worker_id.len() != 32 {
            bail!("worker ID must be a 32-byte ed25519 public key");
        }

        // 2. Verify TEE attestation
        self.verify_attestation(vcr)?;

        // 3. Verify KZG commitment opening
        self.verify_trace_opening(vcr)?;

        // 4. Verify worker signature
        self.verify_signature(vcr)?;

        Ok(())
    }

    /// Verify VCRs from multiple workers (quorum consensus).
    ///
    /// Only VCRs that agree on the majority output are verified and counted
    /// toward quorum. Dissenting VCRs are ignored — a single invalid dissenter
    /// cannot poison a valid quorum. Workers must be unique (Sybil protection).
    pub fn verify_quorum(&self, vcrs: &[VerifiableComputeReceipt]) -> Result<()> {
        if vcrs.len() < self.quorum_size {
            bail!("insufficient quorum: {} < {}", vcrs.len(), self.quorum_size);
        }

        // All VCRs should have same job_id
        let job_id = vcrs[0].job_id;
        for vcr in vcrs {
            if vcr.job_id != job_id {
                bail!("mismatched job IDs in quorum");
            }
        }

        // Sybil protection: reject duplicate worker IDs
        let mut seen_workers = std::collections::HashSet::new();
        for vcr in vcrs {
            if !seen_workers.insert(&vcr.worker_id) {
                bail!("duplicate worker ID in quorum — possible Sybil attack");
            }
        }

        // Find the majority output hash by counting occurrences
        let mut counts: std::collections::HashMap<H256, usize> = std::collections::HashMap::new();
        for vcr in vcrs {
            *counts.entry(vcr.output_hash).or_insert(0) += 1;
        }
        let (&majority_output, &majority_count) = counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .ok_or_else(|| anyhow::anyhow!("empty output set in quorum verification"))?;

        // Check 2/3 consensus on the majority output
        if majority_count * 3 < vcrs.len() * 2 {
            bail!(
                "no consensus: {} / {} agree on majority output",
                majority_count,
                vcrs.len()
            );
        }

        // Only verify VCRs that agree with the majority — dissenters are
        // ignored so one invalid VCR cannot poison the entire quorum.
        let mut verified_count = 0;
        for vcr in vcrs {
            if vcr.output_hash != majority_output {
                continue;
            }
            self.verify(vcr)?;
            verified_count += 1;
        }

        // Ensure enough verified VCRs meet the quorum threshold
        if verified_count < self.quorum_size {
            bail!(
                "insufficient verified quorum: {} < {}",
                verified_count,
                self.quorum_size
            );
        }

        Ok(())
    }

    fn verify_attestation(&self, vcr: &VerifiableComputeReceipt) -> Result<()> {
        let report: AttestationReport = serde_json::from_slice(&vcr.tee_attestation)
            .context("invalid tee_attestation payload (expected JSON AttestationReport)")?;
        let now = current_timestamp();
        self.tee_verifier
            .verify(&report, now)
            .context("TEE attestation verification failed")
    }

    fn verify_trace_opening(&self, vcr: &VerifiableComputeReceipt) -> Result<()> {
        let commitment = KzgCommitment {
            commitment: vcr.trace_commitment.clone(),
        };
        let proof = KzgProof {
            proof: vcr.trace_proof.clone(),
            evaluation: vcr.trace_evaluation.clone(),
        };

        let point: [u8; 32] = vcr
            .trace_point
            .as_slice()
            .try_into()
            .context("trace_point must be 32 bytes")?;
        let valid = self
            .kzg_verifier
            .verify(&commitment, &proof, &point)
            .context("KZG trace proof verification failed")?;
        anyhow::ensure!(valid, "KZG trace proof verification returned false");
        Ok(())
    }

    fn verify_signature(&self, vcr: &VerifiableComputeReceipt) -> Result<()> {
        if vcr.signature.is_empty() {
            bail!("empty signature");
        }

        let message = vcr.signing_message()?;
        ed25519::verify(&vcr.worker_id, &message, &vcr.signature)
            .map_err(|e| anyhow::anyhow!("signature verification failed: {e}"))
    }

    pub fn set_quorum_size(&mut self, size: usize) {
        self.quorum_size = size;
    }

    pub fn challenge_window(&self) -> u64 {
        self.challenge_window
    }
}

impl VerifiableComputeReceipt {
    /// Compute the deterministic signing message using direct hash construction.
    /// This avoids bincode's non-canonical serialization which could differ across versions.
    fn signing_message(&self) -> Result<Vec<u8>> {
        let mut hasher = Sha256::new();
        hasher.update(b"VCR-v1"); // Version domain separator
        hasher.update(self.job_id.as_bytes());
        hasher.update(&self.worker_id);
        hasher.update(self.model_hash.as_bytes());
        hasher.update(self.input_hash.as_bytes());
        hasher.update(self.output_hash.as_bytes());
        hasher.update(&self.trace_commitment);
        hasher.update(&self.trace_proof);
        hasher.update(&self.trace_evaluation);
        hasher.update(&self.trace_point);
        hasher.update(&self.tee_attestation);
        hasher.update(self.timestamp.to_le_bytes());
        Ok(hasher.finalize().to_vec())
    }
}

impl Default for VcrValidator {
    fn default() -> Self {
        Self::new_for_test()
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_verifiers_tee::TeeType;

    fn create_test_vcr(worker: &Keypair, output: u8) -> VerifiableComputeReceipt {
        let report = AttestationReport {
            tee_type: TeeType::Simulation,
            measurement: vec![1u8; 48],
            nonce: vec![2u8; 32],
            timestamp: current_timestamp(),
            signature: vec![3u8; 64],
            cert_chain: vec![vec![4u8; 16]],
        };

        // Create valid KZG commitment/proof using the real verifier
        let kzg = aether_crypto_kzg::KzgVerifier::new_insecure_test(16);
        let mut coeffs = [[0u8; 32]; 2];
        coeffs[0][0] = 3;
        coeffs[1][0] = 1;
        let commitment = kzg.commit(&coeffs).unwrap();
        let mut z = [0u8; 32];
        z[0] = 4;
        let proof = kzg.create_proof(&coeffs, &z).unwrap();

        let mut vcr = VerifiableComputeReceipt {
            job_id: H256::zero(),
            worker_id: worker.public_key(),
            model_hash: H256::zero(),
            input_hash: H256::zero(),
            output_hash: H256::from_slice(&[output; 32]).unwrap(),
            trace_commitment: commitment.commitment,
            trace_proof: proof.proof,
            trace_evaluation: proof.evaluation,
            trace_point: z.to_vec(),
            tee_attestation: serde_json::to_vec(&report).unwrap(),
            timestamp: current_timestamp(),
            signature: Vec::new(),
        };

        let msg = vcr.signing_message().unwrap();
        vcr.signature = worker.sign(&msg);
        vcr
    }

    #[test]
    fn test_verify_single_vcr() {
        let validator = VcrValidator::new_for_test();
        let worker = Keypair::generate();
        let vcr = create_test_vcr(&worker, 5);

        assert!(validator.verify(&vcr).is_ok());
    }

    #[test]
    fn test_quorum_consensus() {
        let validator = VcrValidator::new_for_test();

        // 3 workers, all agree
        let vcrs = vec![
            create_test_vcr(&Keypair::generate(), 5),
            create_test_vcr(&Keypair::generate(), 5),
            create_test_vcr(&Keypair::generate(), 5),
        ];

        assert!(validator.verify_quorum(&vcrs).is_ok());
    }

    #[test]
    fn test_insufficient_quorum() {
        let validator = VcrValidator::new_for_test();

        // Only 2 workers (need 3)
        let vcrs = vec![
            create_test_vcr(&Keypair::generate(), 5),
            create_test_vcr(&Keypair::generate(), 5),
        ];

        assert!(validator.verify_quorum(&vcrs).is_err());
    }

    #[test]
    fn test_no_consensus() {
        let validator = VcrValidator::new_for_test();

        // 3 workers, no agreement
        let vcrs = vec![
            create_test_vcr(&Keypair::generate(), 5),
            create_test_vcr(&Keypair::generate(), 6),
            create_test_vcr(&Keypair::generate(), 7),
        ];

        assert!(validator.verify_quorum(&vcrs).is_err());
    }

    #[test]
    fn test_mismatched_job_ids() {
        let validator = VcrValidator::new_for_test();

        let mut vcrs = vec![
            create_test_vcr(&Keypair::generate(), 5),
            create_test_vcr(&Keypair::generate(), 5),
            create_test_vcr(&Keypair::generate(), 5),
        ];

        // Change job_id of second VCR
        vcrs[1].job_id = H256::from_slice(&[1u8; 32]).unwrap();

        assert!(validator.verify_quorum(&vcrs).is_err());
    }

    #[test]
    fn test_quorum_not_poisoned_by_dissenter() {
        // A dissenting VCR with an invalid signature should NOT cause the
        // quorum to fail — only majority-agreeing VCRs are verified.
        let validator = VcrValidator::new_for_test();

        let mut vcrs = vec![
            create_test_vcr(&Keypair::generate(), 5), // agrees
            create_test_vcr(&Keypair::generate(), 5), // agrees
            create_test_vcr(&Keypair::generate(), 5), // agrees
        ];

        // Add a dissenter with a corrupted signature
        let mut bad_vcr = create_test_vcr(&Keypair::generate(), 99);
        bad_vcr.signature[0] ^= 0xFF;
        vcrs.push(bad_vcr);

        // Should succeed — the bad dissenter is ignored
        assert!(
            validator.verify_quorum(&vcrs).is_ok(),
            "valid quorum should not be poisoned by invalid dissenter"
        );
    }

    #[test]
    fn test_quorum_rejects_sybil_duplicate_workers() {
        let validator = VcrValidator::new_for_test();
        let worker = Keypair::generate();

        // Same worker submits 3 identical VCRs — Sybil attack
        let vcrs = vec![
            create_test_vcr(&worker, 5),
            create_test_vcr(&worker, 5),
            create_test_vcr(&worker, 5),
        ];

        let err = validator.verify_quorum(&vcrs).unwrap_err();
        assert!(
            err.to_string().contains("duplicate worker"),
            "should reject Sybil: {}",
            err
        );
    }

    #[test]
    fn test_quorum_finds_true_majority() {
        // If vcrs[0] is in the minority, the quorum should still find
        // and use the actual majority output.
        let validator = VcrValidator::new_for_test();

        let vcrs = vec![
            create_test_vcr(&Keypair::generate(), 99), // minority (first!)
            create_test_vcr(&Keypair::generate(), 5),  // majority
            create_test_vcr(&Keypair::generate(), 5),  // majority
            create_test_vcr(&Keypair::generate(), 5),  // majority
        ];

        assert!(
            validator.verify_quorum(&vcrs).is_ok(),
            "should succeed using the actual majority, not vcrs[0]"
        );
    }

    #[test]
    fn test_rejects_bad_signature() {
        let validator = VcrValidator::new_for_test();
        let worker = Keypair::generate();
        let mut vcr = create_test_vcr(&worker, 5);
        vcr.signature[0] ^= 0x01;

        assert!(validator.verify(&vcr).is_err());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_verifiers_tee::TeeType;
    use proptest::prelude::*;

    /// Build a valid VCR signed by `worker` with specified output byte.
    fn make_vcr(worker: &Keypair, output: u8) -> VerifiableComputeReceipt {
        let report = aether_verifiers_tee::AttestationReport {
            tee_type: TeeType::Simulation,
            measurement: vec![1u8; 48],
            nonce: vec![2u8; 32],
            timestamp: current_timestamp(),
            signature: vec![3u8; 64],
            cert_chain: vec![vec![4u8; 16]],
        };

        let kzg = aether_crypto_kzg::KzgVerifier::new_insecure_test(16);
        let mut coeffs = [[0u8; 32]; 2];
        coeffs[0][0] = 3;
        coeffs[1][0] = 1;
        let commitment = kzg.commit(&coeffs).unwrap();
        let mut z = [0u8; 32];
        z[0] = 4;
        let proof = kzg.create_proof(&coeffs, &z).unwrap();

        let mut vcr = VerifiableComputeReceipt {
            job_id: H256::zero(),
            worker_id: worker.public_key(),
            model_hash: H256::zero(),
            input_hash: H256::zero(),
            output_hash: H256::from_slice(&[output; 32]).unwrap(),
            trace_commitment: commitment.commitment,
            trace_proof: proof.proof,
            trace_evaluation: proof.evaluation,
            trace_point: z.to_vec(),
            tee_attestation: serde_json::to_vec(&report).unwrap(),
            timestamp: current_timestamp(),
            signature: Vec::new(),
        };

        let msg = vcr.signing_message().unwrap();
        vcr.signature = worker.sign(&msg);
        vcr
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// A valid VCR always passes single-VCR verification.
        #[test]
        fn valid_vcr_always_verifies(output in 1u8..=255u8) {
            let validator = VcrValidator::new_for_test();
            let worker = Keypair::generate();
            let vcr = make_vcr(&worker, output);
            prop_assert!(validator.verify(&vcr).is_ok());
        }

        /// Flipping any byte of the signature always causes rejection.
        #[test]
        fn tampered_signature_always_rejected(
            output in 1u8..=255u8,
            byte_idx in 0usize..64usize,
            flip in 1u8..=255u8,
        ) {
            let validator = VcrValidator::new_for_test();
            let worker = Keypair::generate();
            let mut vcr = make_vcr(&worker, output);
            // ensure the signature is long enough
            if byte_idx < vcr.signature.len() {
                vcr.signature[byte_idx] ^= flip;
                prop_assert!(validator.verify(&vcr).is_err());
            }
        }

        /// A short (< 32-byte) worker ID is always rejected.
        #[test]
        fn short_worker_id_rejected(len in 0usize..32usize) {
            let validator = VcrValidator::new_for_test();
            let worker = Keypair::generate();
            let mut vcr = make_vcr(&worker, 7);
            vcr.worker_id = vec![0u8; len];
            prop_assert!(validator.verify(&vcr).is_err());
        }

        /// verify_quorum succeeds when all workers agree on the same output.
        #[test]
        fn quorum_succeeds_on_unanimous_agreement(n_workers in 3usize..=8usize) {
            let validator = VcrValidator::new_for_test();
            let vcrs: Vec<_> = (0..n_workers)
                .map(|_| make_vcr(&Keypair::generate(), 42))
                .collect();
            prop_assert!(validator.verify_quorum(&vcrs).is_ok());
        }

        /// verify_quorum fails when fewer than quorum_size VCRs are provided.
        #[test]
        fn quorum_fails_below_threshold(n in 0usize..=2usize) {
            let validator = VcrValidator::new_for_test(); // quorum_size = 3
            let vcrs: Vec<_> = (0..n)
                .map(|_| make_vcr(&Keypair::generate(), 42))
                .collect();
            prop_assert!(validator.verify_quorum(&vcrs).is_err());
        }

        /// Duplicate worker IDs are rejected as Sybil attacks.
        #[test]
        fn quorum_rejects_duplicate_worker_ids(output in 1u8..=255u8) {
            let validator = VcrValidator::new_for_test();
            let worker = Keypair::generate();
            // Three VCRs from the same worker — Sybil attack
            let vcrs = vec![
                make_vcr(&worker, output),
                make_vcr(&worker, output),
                make_vcr(&worker, output),
            ];
            prop_assert!(validator.verify_quorum(&vcrs).is_err());
        }

        /// signing_message is deterministic: same VCR fields → same bytes.
        #[test]
        fn signing_message_is_deterministic(output in 1u8..=255u8) {
            let worker = Keypair::generate();
            let vcr = make_vcr(&worker, output);
            let msg1 = vcr.signing_message().unwrap();
            let msg2 = vcr.signing_message().unwrap();
            prop_assert_eq!(msg1, msg2);
        }

        /// signing_message changes when output_hash changes.
        #[test]
        fn signing_message_differs_on_output_change(
            output1 in 1u8..=127u8,
            output2 in 128u8..=255u8,
        ) {
            let worker = Keypair::generate();
            let mut vcr = make_vcr(&worker, output1);
            let msg1 = vcr.signing_message().unwrap();
            vcr.output_hash = H256::from_slice(&[output2; 32]).unwrap();
            let msg2 = vcr.signing_message().unwrap();
            prop_assert_ne!(msg1, msg2);
        }

        /// Quorum with a dissenter minority (1 of 4 disagrees) still succeeds.
        #[test]
        fn quorum_tolerates_single_dissenter(
            majority_output in 1u8..=100u8,
            minority_output in 101u8..=200u8,
        ) {
            let validator = VcrValidator::new_for_test(); // quorum_size = 3
            let vcrs = vec![
                make_vcr(&Keypair::generate(), majority_output),
                make_vcr(&Keypair::generate(), majority_output),
                make_vcr(&Keypair::generate(), majority_output),
                make_vcr(&Keypair::generate(), minority_output), // dissenter
            ];
            // 3-of-4 agree — should still satisfy quorum
            prop_assert!(validator.verify_quorum(&vcrs).is_ok());
        }

        /// VCR with empty trace_commitment is rejected (KZG verification fails).
        #[test]
        fn empty_trace_commitment_rejected(output in 1u8..=255u8) {
            let validator = VcrValidator::new_for_test();
            let worker = Keypair::generate();
            let mut vcr = make_vcr(&worker, output);
            vcr.trace_commitment = Vec::new();
            // Must re-sign after mutation so rejection is from KZG, not signature
            let msg = vcr.signing_message().unwrap();
            vcr.signature = worker.sign(&msg);
            prop_assert!(validator.verify(&vcr).is_err());
        }
    }
}
