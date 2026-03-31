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

    /// Verify VCRs from multiple workers (quorum consensus)
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

        // All VCRs should agree on output
        let output_hash = vcrs[0].output_hash;
        let mut agreement_count = 0;

        for vcr in vcrs {
            if vcr.output_hash == output_hash {
                agreement_count += 1;
            }
        }

        // Check 2/3 consensus
        if agreement_count * 3 < vcrs.len() * 2 {
            bail!("no consensus: {} / {} agree", agreement_count, vcrs.len());
        }

        // Verify each VCR individually
        for vcr in vcrs {
            self.verify(vcr)?;
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
        hasher.update(&self.timestamp.to_le_bytes());
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
    fn test_rejects_bad_signature() {
        let validator = VcrValidator::new_for_test();
        let worker = Keypair::generate();
        let mut vcr = create_test_vcr(&worker, 5);
        vcr.signature[0] ^= 0x01;

        assert!(validator.verify(&vcr).is_err());
    }
}
