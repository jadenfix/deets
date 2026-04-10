// ============================================================================
// AETHER JOB ESCROW - AI Inference Job Management
// ============================================================================
// PURPOSE: Escrow AIC tokens for AI inference requests
//
// FLOW:
// 1. User posts job with AIC deposit
// 2. Provider accepts job
// 3. Provider submits result + VCR
// 4. Validators verify VCR
// 5. Escrow releases payment (burns AIC)
// 6. User receives result
//
// JOB STATES:
// - Posted: Awaiting provider
// - Accepted: Provider working
// - Submitted: Result pending verification
// - Verified: VCR confirmed, payment released
// - Disputed: Challenge active
// - Completed: Final state
// - Cancelled: Refunded
//
// SECURITY:
// - VCR verification required
// - Challenge period (10 slots)
// - Reputation scoring
// - Slashing for invalid results
// ============================================================================

use aether_types::{Address, H256};
use aether_verifiers_vcr::{VcrValidator, VerifiableComputeReceipt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    Posted,
    Accepted,
    Submitted,
    Verified,
    Disputed,
    Completed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub job_id: H256,
    pub requester: Address,
    pub provider: Option<Address>,
    pub model_hash: H256,
    pub input_hash: H256,
    pub output_hash: Option<H256>,
    pub vcr_proof: Option<Vec<u8>>,
    pub payment: u128,
    pub status: JobStatus,
    pub posted_slot: u64,
    pub deadline_slot: u64,
    pub challenge_end_slot: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobEscrowState {
    pub jobs: HashMap<H256, Job>,
    pub provider_reputation: HashMap<Address, i32>,
    pub requester_escrow: HashMap<Address, u128>,
    pub provider_claimable: HashMap<Address, u128>,
    pub total_jobs: u64,
    pub completed_jobs: u64,
}

impl JobEscrowState {
    pub fn new() -> Self {
        JobEscrowState {
            jobs: HashMap::new(),
            provider_reputation: HashMap::new(),
            requester_escrow: HashMap::new(),
            provider_claimable: HashMap::new(),
            total_jobs: 0,
            completed_jobs: 0,
        }
    }

    /// Post a new job
    #[allow(clippy::too_many_arguments)]
    pub fn post_job(
        &mut self,
        job_id: H256,
        requester: Address,
        model_hash: H256,
        input_hash: H256,
        payment: u128,
        current_slot: u64,
        deadline_slots: u64,
    ) -> Result<(), String> {
        if self.jobs.contains_key(&job_id) {
            return Err("job already exists".to_string());
        }

        if payment == 0 {
            return Err("payment must be non-zero".to_string());
        }

        let job = Job {
            job_id,
            requester,
            provider: None,
            model_hash,
            input_hash,
            output_hash: None,
            vcr_proof: None,
            payment,
            status: JobStatus::Posted,
            posted_slot: current_slot,
            deadline_slot: current_slot
                .checked_add(deadline_slots)
                .ok_or_else(|| "slot overflow in deadline calculation".to_string())?,
            challenge_end_slot: None,
        };

        self.jobs.insert(job_id, job);
        let escrowed = self.requester_escrow.entry(requester).or_insert(0);
        *escrowed = escrowed
            .checked_add(payment)
            .ok_or("requester escrow overflow")?;
        self.total_jobs = self
            .total_jobs
            .checked_add(1)
            .ok_or("total_jobs overflow")?;

        Ok(())
    }

    /// Minimum provider reputation required to accept a job.
    ///
    /// Providers whose reputation score is at or below this threshold have been
    /// penalised sufficiently that they are barred from taking new work.  The
    /// coordinator independently bans providers at -100, but the on-chain
    /// escrow enforces the same floor so a compromised off-chain coordinator
    /// cannot bypass it.
    pub const MIN_PROVIDER_REPUTATION: i32 = -50;

    /// Provider accepts job
    pub fn accept_job(&mut self, job_id: H256, provider: Address) -> Result<(), String> {
        // Reject providers whose reputation is too low.
        let reputation = self.get_provider_reputation(&provider);
        if reputation <= Self::MIN_PROVIDER_REPUTATION {
            return Err(format!(
                "provider reputation {} is too low to accept jobs (minimum {})",
                reputation,
                Self::MIN_PROVIDER_REPUTATION
            ));
        }

        let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;

        if job.status != JobStatus::Posted {
            return Err("job not available".to_string());
        }

        // A requester must not be able to act as provider for their own job —
        // doing so would let them steal the escrowed payment.
        if provider == job.requester {
            return Err("provider cannot be the same address as the job requester".to_string());
        }

        job.provider = Some(provider);
        job.status = JobStatus::Accepted;

        Ok(())
    }

    /// Provider submits result
    pub fn submit_result(
        &mut self,
        job_id: H256,
        provider: Address,
        output_hash: H256,
        vcr_proof: Vec<u8>,
        current_slot: u64,
    ) -> Result<(), String> {
        let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;

        if job.provider != Some(provider) {
            return Err("not job provider".to_string());
        }

        if job.status != JobStatus::Accepted {
            return Err("invalid job status".to_string());
        }

        if current_slot > job.deadline_slot {
            return Err("deadline passed".to_string());
        }

        job.output_hash = Some(output_hash);
        job.vcr_proof = Some(vcr_proof);
        job.status = JobStatus::Submitted;
        job.challenge_end_slot = Some(
            current_slot
                .checked_add(10)
                .ok_or_else(|| "slot overflow in challenge period calculation".to_string())?,
        ); // 10 slot challenge period

        Ok(())
    }

    /// Verify and complete job.
    ///
    /// `vcr_validator` is used to cryptographically verify the stored VCR proof
    /// (TEE attestation + KZG trace commitment + worker signature).  The job
    /// transitions to `Completed` only when verification passes.
    pub fn verify_job(
        &mut self,
        job_id: H256,
        current_slot: u64,
        vcr_validator: &VcrValidator,
    ) -> Result<Option<(Address, u128)>, String> {
        let (requester, provider, payment) = {
            let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;

            if job.status != JobStatus::Submitted {
                return Err("job not submitted".to_string());
            }

            // Check challenge period ended
            if let Some(challenge_end) = job.challenge_end_slot {
                if current_slot <= challenge_end {
                    return Err("challenge period not ended".to_string());
                }
            }

            // Cryptographically verify the VCR proof (TEE attestation, KZG trace
            // commitment, and worker signature) before releasing payment.
            let proof_bytes = job.vcr_proof.as_deref().ok_or("missing VCR proof")?;
            let receipt: VerifiableComputeReceipt = serde_json::from_slice(proof_bytes)
                .map_err(|e| format!("invalid VCR proof encoding: {e}"))?;
            vcr_validator
                .verify(&receipt)
                .map_err(|e| format!("VCR proof verification failed: {e}"))?;

            let provider = job.provider.ok_or("job has no provider")?;
            let requester = job.requester;
            let payment = job.payment;
            (requester, provider, payment)
        };

        let escrowed = self
            .requester_escrow
            .get_mut(&requester)
            .ok_or("missing requester escrow balance")?;
        if *escrowed < payment {
            return Err("insufficient requester escrow balance".to_string());
        }
        *escrowed = escrowed.checked_sub(payment).ok_or("escrow underflow")?;
        let remove_requester_escrow = *escrowed == 0;
        if remove_requester_escrow {
            self.requester_escrow.remove(&requester);
        }
        let claimable = self.provider_claimable.entry(provider).or_insert(0);
        *claimable = claimable
            .checked_add(payment)
            .ok_or("provider claimable overflow")?;
        let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;
        job.status = JobStatus::Completed;
        let rep = self.provider_reputation.entry(provider).or_insert(0);
        *rep = rep.checked_add(1).ok_or("reputation overflow")?;
        self.completed_jobs = self
            .completed_jobs
            .checked_add(1)
            .ok_or("completed_jobs overflow")?;

        Ok(Some((provider, payment)))
    }

    /// Challenge a result.
    ///
    /// Only the job requester can challenge a submitted result.
    /// This puts the job into Disputed status, preventing automatic verification.
    pub fn challenge_job(&mut self, job_id: H256, challenger: Address) -> Result<(), String> {
        let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;

        if job.status != JobStatus::Submitted {
            return Err("cannot challenge job".to_string());
        }

        // Only the job requester can challenge
        if challenger != job.requester {
            return Err("only job requester can challenge".to_string());
        }

        job.status = JobStatus::Disputed;

        Ok(())
    }

    /// Cancel job (refund requester)
    pub fn cancel_job(&mut self, job_id: H256, caller: Address) -> Result<(), String> {
        let (requester, payment) = {
            let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;

            if caller != job.requester {
                return Err("not job requester".to_string());
            }

            if job.status != JobStatus::Posted {
                return Err("cannot cancel job".to_string());
            }

            let requester = job.requester;
            let payment = job.payment;
            (requester, payment)
        };

        let escrowed = self
            .requester_escrow
            .get_mut(&requester)
            .ok_or("missing requester escrow balance")?;
        if *escrowed < payment {
            return Err("insufficient requester escrow balance".to_string());
        }
        *escrowed = escrowed.checked_sub(payment).ok_or("escrow underflow")?;
        let remove_requester_escrow = *escrowed == 0;
        if remove_requester_escrow {
            self.requester_escrow.remove(&requester);
        }
        let job = self.jobs.get_mut(&job_id).ok_or("job not found")?;
        job.status = JobStatus::Cancelled;

        Ok(())
    }

    pub fn get_job(&self, job_id: &H256) -> Option<&Job> {
        self.jobs.get(job_id)
    }

    pub fn get_provider_reputation(&self, provider: &Address) -> i32 {
        self.provider_reputation.get(provider).copied().unwrap_or(0)
    }

    pub fn escrowed_balance_of(&self, requester: &Address) -> u128 {
        self.requester_escrow.get(requester).copied().unwrap_or(0)
    }

    pub fn claimable_balance_of(&self, provider: &Address) -> u128 {
        self.provider_claimable.get(provider).copied().unwrap_or(0)
    }
}

impl Default for JobEscrowState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_verifiers_tee::{AttestationReport, TeeType};

    fn addr(n: u8) -> Address {
        Address::from_slice(&[n; 20]).unwrap()
    }

    /// Build a valid serialized VCR for use in tests.
    fn make_valid_vcr_bytes(job_id: H256) -> Vec<u8> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let worker = Keypair::generate();
        let report = AttestationReport {
            tee_type: TeeType::Simulation,
            measurement: vec![1u8; 48],
            nonce: vec![2u8; 32],
            timestamp: now,
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
            job_id,
            worker_id: worker.public_key(),
            model_hash: H256::zero(),
            input_hash: H256::zero(),
            output_hash: H256::zero(),
            trace_commitment: commitment.commitment,
            trace_proof: proof.proof,
            trace_evaluation: proof.evaluation,
            trace_point: z.to_vec(),
            tee_attestation: serde_json::to_vec(&report).unwrap(),
            timestamp: now,
            signature: Vec::new(),
        };
        // Sign using the same signing_message logic exposed via verify
        // (we replicate the hash construction used inside VerifiableComputeReceipt)
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"VCR-v1");
        hasher.update(vcr.job_id.as_bytes());
        hasher.update(&vcr.worker_id);
        hasher.update(vcr.model_hash.as_bytes());
        hasher.update(vcr.input_hash.as_bytes());
        hasher.update(vcr.output_hash.as_bytes());
        hasher.update(&vcr.trace_commitment);
        hasher.update(&vcr.trace_proof);
        hasher.update(&vcr.trace_evaluation);
        hasher.update(&vcr.trace_point);
        hasher.update(&vcr.tee_attestation);
        hasher.update(vcr.timestamp.to_le_bytes());
        let msg: Vec<u8> = hasher.finalize().to_vec();
        vcr.signature = worker.sign(&msg);
        serde_json::to_vec(&vcr).unwrap()
    }

    #[test]
    fn test_post_job() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Posted);
        assert_eq!(job.payment, 1000);
        assert_eq!(state.escrowed_balance_of(&addr(1)), 1000);
    }

    #[test]
    fn test_accept_job() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();
        state.accept_job(job_id, addr(2)).unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Accepted);
        assert_eq!(job.provider, Some(addr(2)));
    }

    #[test]
    fn test_submit_and_verify() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();
        let vcr_bytes = make_valid_vcr_bytes(job_id);
        let validator = VcrValidator::new_for_test();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();
        state.accept_job(job_id, addr(2)).unwrap();
        state
            .submit_result(job_id, addr(2), H256::zero(), vcr_bytes, 150)
            .unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Submitted);

        // Verify after challenge period
        let result = state.verify_job(job_id, 200, &validator).unwrap();
        assert!(result.is_some());
        let (provider, payment) = result.unwrap();
        assert_eq!(provider, addr(2));
        assert_eq!(payment, 1000);

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(state.escrowed_balance_of(&addr(1)), 0);
        assert_eq!(state.claimable_balance_of(&addr(2)), 1000);
        assert_eq!(state.get_provider_reputation(&addr(2)), 1);
    }

    #[test]
    fn test_verify_job_rejects_invalid_vcr() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();
        let validator = VcrValidator::new_for_test();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();
        state.accept_job(job_id, addr(2)).unwrap();
        // Submit garbage bytes as the VCR proof
        state
            .submit_result(
                job_id,
                addr(2),
                H256::zero(),
                vec![0xde, 0xad, 0xbe, 0xef],
                150,
            )
            .unwrap();

        let err = state.verify_job(job_id, 200, &validator).unwrap_err();
        assert!(
            err.contains("invalid VCR proof encoding")
                || err.contains("VCR proof verification failed"),
            "unexpected error: {err}"
        );
        // Job must remain Submitted (not completed) after a failed verification
        assert_eq!(state.get_job(&job_id).unwrap().status, JobStatus::Submitted);
    }

    #[test]
    fn test_accept_job_requester_cannot_be_provider() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();

        // addr(1) is the requester — they must not be allowed to accept their own job.
        let err = state.accept_job(job_id, addr(1)).unwrap_err();
        assert!(
            err.contains("provider cannot be the same address as the job requester"),
            "unexpected error: {err}"
        );

        // Job should still be Posted.
        assert_eq!(state.get_job(&job_id).unwrap().status, JobStatus::Posted);
    }

    #[test]
    fn test_accept_job_low_reputation_blocked() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();

        // Drive addr(2) reputation to -51 (below threshold).
        *state.provider_reputation.entry(addr(2)).or_insert(0) = -51;

        let err = state.accept_job(job_id, addr(2)).unwrap_err();
        assert!(
            err.contains("reputation") && err.contains("too low"),
            "unexpected error: {err}"
        );

        // A provider at exactly MIN_PROVIDER_REPUTATION is also blocked.
        *state.provider_reputation.entry(addr(2)).or_insert(0) =
            JobEscrowState::MIN_PROVIDER_REPUTATION;
        let err2 = state.accept_job(job_id, addr(2)).unwrap_err();
        assert!(err2.contains("too low"), "unexpected error: {err2}");
    }

    #[test]
    fn test_accept_job_good_reputation_allowed() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000)
            .unwrap();

        // addr(2) has reputation -49, one above the threshold — should be allowed.
        *state.provider_reputation.entry(addr(2)).or_insert(0) = -49;
        state.accept_job(job_id, addr(2)).unwrap();
        assert_eq!(state.get_job(&job_id).unwrap().status, JobStatus::Accepted);
    }

    #[test]
    fn test_cancel_job_releases_requester_escrow() {
        let mut state = JobEscrowState::new();
        let job_id = H256::from_slice(&[1u8; 32]).unwrap();

        state
            .post_job(job_id, addr(1), H256::zero(), H256::zero(), 750, 100, 1000)
            .unwrap();
        assert_eq!(state.escrowed_balance_of(&addr(1)), 750);

        state.cancel_job(job_id, addr(1)).unwrap();
        assert_eq!(state.escrowed_balance_of(&addr(1)), 0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_addr() -> impl Strategy<Value = Address> {
        prop::array::uniform20(any::<u8>()).prop_map(|b| Address::from_slice(&b).unwrap())
    }

    fn arb_h256() -> impl Strategy<Value = H256> {
        prop::array::uniform32(any::<u8>()).prop_map(|b| H256::from_slice(&b).unwrap())
    }

    proptest! {
        /// Duplicate job IDs are rejected — posting the same job_id twice fails.
        #[test]
        fn duplicate_job_rejected(
            job_id in arb_h256(),
            requester in arb_addr(),
            payment in 1u128..=u128::MAX / 2,
        ) {
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            let err = state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap_err();
            prop_assert!(err.contains("already exists"), "expected duplicate-job error, got: {err}");
        }

        /// Zero payment is always rejected.
        #[test]
        fn zero_payment_rejected(
            job_id in arb_h256(),
            requester in arb_addr(),
        ) {
            let mut state = JobEscrowState::new();
            let err = state
                .post_job(job_id, requester, H256::zero(), H256::zero(), 0, 0, 1000)
                .unwrap_err();
            prop_assert!(err.contains("non-zero"), "expected payment error, got: {err}");
        }

        /// Posting a job puts the exact payment amount in requester escrow.
        #[test]
        fn post_job_escrow_equals_payment(
            job_id in arb_h256(),
            requester in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
        ) {
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            prop_assert_eq!(state.escrowed_balance_of(&requester), payment);
        }

        /// Cancel releases the exact escrowed amount; no funds remain.
        #[test]
        fn cancel_releases_full_escrow(
            job_id in arb_h256(),
            requester in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
        ) {
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            state.cancel_job(job_id, requester).unwrap();
            prop_assert_eq!(state.escrowed_balance_of(&requester), 0);
        }

        /// Third-party cannot cancel another requester's job.
        #[test]
        fn non_requester_cannot_cancel(
            job_id in arb_h256(),
            requester in arb_addr(),
            stranger in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
        ) {
            prop_assume!(requester != stranger);
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            let err = state.cancel_job(job_id, stranger).unwrap_err();
            prop_assert!(
                err.contains("not job requester"),
                "expected requester-check error, got: {err}"
            );
            // Job must remain Posted.
            prop_assert_eq!(&state.get_job(&job_id).unwrap().status, &JobStatus::Posted);
        }

        /// Provider cannot be the same address as the job requester.
        #[test]
        fn requester_cannot_self_accept(
            job_id in arb_h256(),
            requester in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
        ) {
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            let err = state.accept_job(job_id, requester).unwrap_err();
            prop_assert!(
                err.contains("provider cannot be the same address"),
                "expected self-accept error, got: {err}"
            );
        }

        /// A provider with reputation > MIN_PROVIDER_REPUTATION can always accept.
        #[test]
        fn provider_with_ok_reputation_accepted(
            job_id in arb_h256(),
            requester in arb_addr(),
            provider in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
            rep in (JobEscrowState::MIN_PROVIDER_REPUTATION + 1)..=1000i32,
        ) {
            prop_assume!(requester != provider);
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            *state.provider_reputation.entry(provider).or_insert(0) = rep;
            state.accept_job(job_id, provider).unwrap();
            prop_assert_eq!(&state.get_job(&job_id).unwrap().status, &JobStatus::Accepted);
        }

        /// A provider at or below MIN_PROVIDER_REPUTATION is always blocked.
        #[test]
        fn provider_at_floor_reputation_blocked(
            job_id in arb_h256(),
            requester in arb_addr(),
            provider in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
            rep in i32::MIN..=JobEscrowState::MIN_PROVIDER_REPUTATION,
        ) {
            prop_assume!(requester != provider);
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            *state.provider_reputation.entry(provider).or_insert(0) = rep;
            let err = state.accept_job(job_id, provider).unwrap_err();
            prop_assert!(
                err.contains("too low"),
                "expected reputation error, got: {err}"
            );
        }

        /// Submit after deadline is always rejected.
        #[test]
        fn submit_after_deadline_rejected(
            job_id in arb_h256(),
            requester in arb_addr(),
            provider in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
            deadline_slots in 1u64..=100,
        ) {
            prop_assume!(requester != provider);
            let mut state = JobEscrowState::new();
            let post_slot = 100u64;
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, post_slot, deadline_slots)
                .unwrap();
            state.accept_job(job_id, provider).unwrap();
            // Submit one slot past the deadline.
            let past_deadline = post_slot + deadline_slots + 1;
            let err = state
                .submit_result(job_id, provider, H256::zero(), vec![], past_deadline)
                .unwrap_err();
            prop_assert!(err.contains("deadline"), "expected deadline error, got: {err}");
        }

        /// Only the requester can challenge a submitted result.
        #[test]
        fn only_requester_can_challenge(
            job_id in arb_h256(),
            requester in arb_addr(),
            provider in arb_addr(),
            stranger in arb_addr(),
            payment in 1u128..=1_000_000_000u128,
        ) {
            prop_assume!(requester != provider);
            prop_assume!(requester != stranger);
            prop_assume!(provider != stranger);
            let mut state = JobEscrowState::new();
            state
                .post_job(job_id, requester, H256::zero(), H256::zero(), payment, 0, 1000)
                .unwrap();
            state.accept_job(job_id, provider).unwrap();
            state
                .submit_result(job_id, provider, H256::zero(), vec![0xab], 50)
                .unwrap();
            // Stranger cannot challenge.
            let err = state.challenge_job(job_id, stranger).unwrap_err();
            prop_assert!(err.contains("requester"), "expected requester-only error, got: {err}");
            // Requester can challenge.
            state.challenge_job(job_id, requester).unwrap();
            prop_assert_eq!(&state.get_job(&job_id).unwrap().status, &JobStatus::Disputed);
        }

        /// total_jobs counter increments exactly once per successful post.
        #[test]
        fn total_jobs_counter_monotone(
            jobs in prop::collection::vec((arb_h256(), arb_addr(), 1u128..=1_000u128), 1..20),
        ) {
            let mut state = JobEscrowState::new();
            let mut expected: u64 = 0;
            let mut seen_ids = std::collections::HashSet::new();
            for (job_id, requester, payment) in &jobs {
                if seen_ids.insert(*job_id) {
                    state
                        .post_job(*job_id, *requester, H256::zero(), H256::zero(), *payment, 0, 1000)
                        .unwrap();
                    expected += 1;
                }
            }
            prop_assert_eq!(state.total_jobs, expected);
        }
    }
}
