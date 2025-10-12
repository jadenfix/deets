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

use serde::{Deserialize, Serialize};
use aether_types::{Address, H256};
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
    pub total_jobs: u64,
    pub completed_jobs: u64,
}

impl JobEscrowState {
    pub fn new() -> Self {
        JobEscrowState {
            jobs: HashMap::new(),
            provider_reputation: HashMap::new(),
            total_jobs: 0,
            completed_jobs: 0,
        }
    }

    /// Post a new job
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
            deadline_slot: current_slot + deadline_slots,
            challenge_end_slot: None,
        };

        self.jobs.insert(job_id, job);
        self.total_jobs += 1;

        Ok(())
    }

    /// Provider accepts job
    pub fn accept_job(&mut self, job_id: H256, provider: Address) -> Result<(), String> {
        let job = self.jobs.get_mut(&job_id)
            .ok_or("job not found")?;

        if job.status != JobStatus::Posted {
            return Err("job not available".to_string());
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
        let job = self.jobs.get_mut(&job_id)
            .ok_or("job not found")?;

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
        job.challenge_end_slot = Some(current_slot + 10); // 10 slot challenge period

        Ok(())
    }

    /// Verify and complete job
    pub fn verify_job(&mut self, job_id: H256, current_slot: u64) -> Result<(), String> {
        let job = self.jobs.get_mut(&job_id)
            .ok_or("job not found")?;

        if job.status != JobStatus::Submitted {
            return Err("job not submitted".to_string());
        }

        // Check challenge period ended
        if let Some(challenge_end) = job.challenge_end_slot {
            if current_slot < challenge_end {
                return Err("challenge period not ended".to_string());
            }
        }

        // In production: verify VCR proof
        // For now: assume valid

        job.status = JobStatus::Verified;

        // Update provider reputation
        if let Some(provider) = job.provider {
            *self.provider_reputation.entry(provider).or_insert(0) += 1;
        }

        self.completed_jobs += 1;

        Ok(())
    }

    /// Challenge a result
    pub fn challenge_job(&mut self, job_id: H256, challenger: Address) -> Result<(), String> {
        let job = self.jobs.get_mut(&job_id)
            .ok_or("job not found")?;

        if job.status != JobStatus::Submitted {
            return Err("cannot challenge job".to_string());
        }

        job.status = JobStatus::Disputed;

        Ok(())
    }

    /// Cancel job (refund requester)
    pub fn cancel_job(&mut self, job_id: H256, caller: Address) -> Result<(), String> {
        let job = self.jobs.get_mut(&job_id)
            .ok_or("job not found")?;

        if caller != job.requester {
            return Err("not job requester".to_string());
        }

        if job.status != JobStatus::Posted {
            return Err("cannot cancel job".to_string());
        }

        job.status = JobStatus::Cancelled;

        Ok(())
    }

    pub fn get_job(&self, job_id: &H256) -> Option<&Job> {
        self.jobs.get(job_id)
    }

    pub fn get_provider_reputation(&self, provider: &Address) -> i32 {
        self.provider_reputation.get(provider).copied().unwrap_or(0)
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

    fn addr(n: u8) -> Address {
        Address::from_slice(&[n; 20]).unwrap()
    }

    #[test]
    fn test_post_job() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state.post_job(
            job_id,
            addr(1),
            H256::zero(),
            H256::zero(),
            1000,
            100,
            1000,
        ).unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Posted);
        assert_eq!(job.payment, 1000);
    }

    #[test]
    fn test_accept_job() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state.post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000).unwrap();
        state.accept_job(job_id, addr(2)).unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Accepted);
        assert_eq!(job.provider, Some(addr(2)));
    }

    #[test]
    fn test_submit_and_verify() {
        let mut state = JobEscrowState::new();
        let job_id = H256::zero();

        state.post_job(job_id, addr(1), H256::zero(), H256::zero(), 1000, 100, 1000).unwrap();
        state.accept_job(job_id, addr(2)).unwrap();
        state.submit_result(job_id, addr(2), H256::zero(), vec![1, 2, 3], 150).unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Submitted);

        // Verify after challenge period
        state.verify_job(job_id, 200).unwrap();

        let job = state.get_job(&job_id).unwrap();
        assert_eq!(job.status, JobStatus::Verified);
        assert_eq!(state.get_provider_reputation(&addr(2)), 1);
    }
}
