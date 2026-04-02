// ============================================================================
// AETHER AI MESH COORDINATOR
// ============================================================================
// PURPOSE: Match jobs with workers, manage reputation, handle disputes
//
// RESPONSIBILITIES:
// - Job assignment (match jobs to capable workers)
// - Worker discovery (find available workers)
// - Reputation tracking (success/failure rates)
// - Dispute resolution (handle challenges)
// - Load balancing (distribute work evenly)
//
// ARCHITECTURE:
// - On-chain state (job escrow, reputation)
// - Off-chain coordination (P2P discovery)
// - Challenge mechanism (verify VCRs)
//
// WORKFLOW:
// 1. User posts job → escrow locks AIC
// 2. Coordinator finds eligible workers
// 3. Workers bid on job (gas price, latency)
// 4. Coordinator assigns to best worker
// 5. Worker executes → submits VCR
// 6. Challenge period (10 slots)
// 7. If no challenge → payment released
// 8. Reputation updated
//
// REPUTATION SCORING:
// - Success rate: completed/attempted
// - Latency: average response time
// - Quality: challenge win rate
// - Uptime: availability percentage
// ============================================================================

use aether_verifiers_tee::{AttestationReport, TeeVerifier};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub worker_id: Vec<u8>,
    pub tee_type: String,
    pub attestation: Vec<u8>,
    pub capabilities: Vec<String>,
    pub reputation_score: i32,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct JobAssignment {
    pub job_id: Vec<u8>,
    pub worker_id: Vec<u8>,
    pub assigned_at: u64,
}

pub struct MeshCoordinator {
    /// Registered workers
    workers: HashMap<Vec<u8>, WorkerInfo>,

    /// Active job assignments
    assignments: HashMap<Vec<u8>, JobAssignment>,

    /// Reputation history
    reputation: HashMap<Vec<u8>, Vec<ReputationEvent>>,

    /// TEE attestation verifier
    tee_verifier: TeeVerifier,
}

#[derive(Debug, Clone)]
pub struct ReputationEvent {
    pub timestamp: u64,
    pub event_type: ReputationEventType,
    pub score_change: i32,
}

#[derive(Debug, Clone)]
pub enum ReputationEventType {
    JobCompleted,
    JobFailed,
    ChallengeWon,
    ChallengeLost,
    Timeout,
}

impl MeshCoordinator {
    pub fn new() -> Self {
        let mut tee_verifier = TeeVerifier::new();
        // Default simulation measurement for dev/test workers.
        tee_verifier.add_approved_measurement(vec![1u8; 48]);
        MeshCoordinator {
            workers: HashMap::new(),
            assignments: HashMap::new(),
            reputation: HashMap::new(),
            tee_verifier,
        }
    }

    pub fn approve_measurement(&mut self, measurement: Vec<u8>) {
        self.tee_verifier.add_approved_measurement(measurement);
    }

    /// Register a new worker
    pub fn register_worker(&mut self, worker: WorkerInfo) -> Result<()> {
        // Verify TEE attestation
        if worker.attestation.is_empty() {
            bail!("missing attestation");
        }
        let report: AttestationReport =
            serde_json::from_slice(&worker.attestation).map_err(|e| {
                anyhow::anyhow!("invalid attestation payload (expected JSON report): {e}")
            })?;
        self.tee_verifier
            .verify(&report, current_timestamp())
            .map_err(|e| anyhow::anyhow!("attestation verification failed: {e}"))?;

        self.workers.insert(worker.worker_id.clone(), worker);

        Ok(())
    }

    /// Find best worker for a job
    pub fn assign_job(
        &mut self,
        job_id: Vec<u8>,
        requirements: &JobRequirements,
    ) -> Result<Vec<u8>> {
        // Find eligible workers
        let mut candidates: Vec<&WorkerInfo> = self
            .workers
            .values()
            .filter(|w| w.available && self.meets_requirements(w, requirements))
            .collect();

        if candidates.is_empty() {
            bail!("no eligible workers");
        }

        // Sort by reputation (best first)
        candidates.sort_by(|a, b| b.reputation_score.cmp(&a.reputation_score));

        let best_worker = candidates[0];

        // Create assignment
        let assignment = JobAssignment {
            job_id: job_id.clone(),
            worker_id: best_worker.worker_id.clone(),
            assigned_at: current_timestamp(),
        };

        // Reject duplicate job assignment
        if self.assignments.contains_key(&job_id) {
            bail!("job already assigned");
        }

        let worker_id = best_worker.worker_id.clone();
        self.assignments.insert(job_id, assignment);

        // Mark worker as occupied so it won't be double-assigned
        if let Some(w) = self.workers.get_mut(&worker_id) {
            w.available = false;
        }

        Ok(worker_id)
    }

    /// Complete a job — release worker back to available pool
    pub fn complete_job(&mut self, job_id: &[u8]) -> Result<Vec<u8>> {
        let assignment = self
            .assignments
            .remove(job_id)
            .ok_or_else(|| anyhow::anyhow!("job not found"))?;

        // Mark worker available again
        if let Some(w) = self.workers.get_mut(&assignment.worker_id) {
            w.available = true;
        }

        Ok(assignment.worker_id)
    }

    /// Cancel a job — release worker without reputation penalty
    pub fn cancel_job(&mut self, job_id: &[u8]) -> Result<Vec<u8>> {
        let assignment = self
            .assignments
            .remove(job_id)
            .ok_or_else(|| anyhow::anyhow!("job not found"))?;

        if let Some(w) = self.workers.get_mut(&assignment.worker_id) {
            w.available = true;
        }

        Ok(assignment.worker_id)
    }

    /// Update worker reputation
    pub fn update_reputation(
        &mut self,
        worker_id: &[u8],
        event_type: ReputationEventType,
    ) -> Result<()> {
        let worker = self
            .workers
            .get_mut(worker_id)
            .ok_or_else(|| anyhow::anyhow!("worker not found"))?;

        let score_change = match event_type {
            ReputationEventType::JobCompleted => 10,
            ReputationEventType::JobFailed => -20,
            ReputationEventType::ChallengeWon => 5,
            ReputationEventType::ChallengeLost => -50,
            ReputationEventType::Timeout => -30,
        };

        worker.reputation_score = (worker.reputation_score + score_change).clamp(-100, 1000);

        // Record event
        let event = ReputationEvent {
            timestamp: current_timestamp(),
            event_type: event_type.clone(),
            score_change,
        };

        self.reputation
            .entry(worker_id.to_vec())
            .or_default()
            .push(event);

        // Ban worker if reputation too low
        if worker.reputation_score <= -100 {
            worker.available = false;
            println!(
                "Worker {:?} banned (low reputation)",
                hex::encode(worker_id)
            );
        }

        Ok(())
    }

    fn meets_requirements(&self, worker: &WorkerInfo, requirements: &JobRequirements) -> bool {
        // Check TEE type
        if !requirements.tee_types.contains(&worker.tee_type) {
            return false;
        }

        // Check reputation
        if worker.reputation_score < requirements.min_reputation {
            return false;
        }

        // Check capabilities
        for required_cap in &requirements.capabilities {
            if !worker.capabilities.contains(required_cap) {
                return false;
            }
        }

        true
    }

    pub fn get_worker(&self, worker_id: &[u8]) -> Option<&WorkerInfo> {
        self.workers.get(worker_id)
    }

    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    pub fn available_worker_count(&self) -> usize {
        self.workers.values().filter(|w| w.available).count()
    }
}

impl Default for MeshCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct JobRequirements {
    pub tee_types: Vec<String>,
    pub capabilities: Vec<String>,
    pub min_reputation: i32,
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
    use aether_verifiers_tee::{AttestationReport, TeeType};

    fn test_worker(id: u8, reputation: i32) -> WorkerInfo {
        let report = AttestationReport {
            tee_type: TeeType::Simulation,
            measurement: vec![1u8; 48],
            nonce: vec![2u8; 32],
            timestamp: current_timestamp(),
            signature: vec![3u8; 64],
            cert_chain: vec![vec![4u8; 16]],
        };
        WorkerInfo {
            worker_id: vec![id],
            tee_type: "sev-snp".to_string(),
            attestation: serde_json::to_vec(&report).unwrap(),
            capabilities: vec!["onnx".to_string()],
            reputation_score: reputation,
            available: true,
        }
    }

    #[test]
    fn test_register_worker() {
        let mut coordinator = MeshCoordinator::new();
        let worker = test_worker(1, 0);

        coordinator.register_worker(worker).unwrap();

        assert_eq!(coordinator.worker_count(), 1);
    }

    #[test]
    fn test_assign_job() {
        let mut coordinator = MeshCoordinator::new();

        coordinator.register_worker(test_worker(1, 100)).unwrap();
        coordinator.register_worker(test_worker(2, 50)).unwrap();

        let requirements = JobRequirements {
            tee_types: vec!["sev-snp".to_string()],
            capabilities: vec!["onnx".to_string()],
            min_reputation: 0,
        };

        let assigned = coordinator
            .assign_job(vec![1, 2, 3], &requirements)
            .unwrap();

        // Should assign to worker 1 (higher reputation)
        assert_eq!(assigned, vec![1]);
    }

    #[test]
    fn test_reputation_update() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, 0)).unwrap();

        coordinator
            .update_reputation(&[1], ReputationEventType::JobCompleted)
            .unwrap();

        let worker = coordinator.get_worker(&[1]).unwrap();
        assert_eq!(worker.reputation_score, 10);
    }

    #[test]
    fn test_assign_job_marks_worker_unavailable() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, 100)).unwrap();

        let reqs = JobRequirements {
            tee_types: vec!["sev-snp".to_string()],
            capabilities: vec!["onnx".to_string()],
            min_reputation: 0,
        };

        coordinator.assign_job(vec![1], &reqs).unwrap();

        // Worker should now be unavailable
        assert!(!coordinator.get_worker(&[1]).unwrap().available);
        assert_eq!(coordinator.available_worker_count(), 0);

        // Second job should fail — no available workers
        let err = coordinator.assign_job(vec![2], &reqs).unwrap_err();
        assert!(err.to_string().contains("no eligible workers"));
    }

    #[test]
    fn test_duplicate_job_assignment_rejected() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, 100)).unwrap();
        coordinator.register_worker(test_worker(2, 50)).unwrap();

        let reqs = JobRequirements {
            tee_types: vec!["sev-snp".to_string()],
            capabilities: vec!["onnx".to_string()],
            min_reputation: 0,
        };

        coordinator.assign_job(vec![1], &reqs).unwrap();
        let err = coordinator.assign_job(vec![1], &reqs).unwrap_err();
        assert!(err.to_string().contains("job already assigned"));
    }

    #[test]
    fn test_complete_job_releases_worker() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, 100)).unwrap();

        let reqs = JobRequirements {
            tee_types: vec!["sev-snp".to_string()],
            capabilities: vec!["onnx".to_string()],
            min_reputation: 0,
        };

        coordinator.assign_job(vec![1], &reqs).unwrap();
        assert_eq!(coordinator.available_worker_count(), 0);

        let worker_id = coordinator.complete_job(&[1]).unwrap();
        assert_eq!(worker_id, vec![1]);
        assert_eq!(coordinator.available_worker_count(), 1);

        // Can now assign again
        coordinator.assign_job(vec![2], &reqs).unwrap();
    }

    #[test]
    fn test_cancel_job_releases_worker() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, 100)).unwrap();

        let reqs = JobRequirements {
            tee_types: vec!["sev-snp".to_string()],
            capabilities: vec!["onnx".to_string()],
            min_reputation: 0,
        };

        coordinator.assign_job(vec![1], &reqs).unwrap();
        coordinator.cancel_job(&[1]).unwrap();
        assert!(coordinator.get_worker(&[1]).unwrap().available);
    }

    #[test]
    fn test_complete_nonexistent_job_fails() {
        let mut coordinator = MeshCoordinator::new();
        let err = coordinator.complete_job(&[99]).unwrap_err();
        assert!(err.to_string().contains("job not found"));
    }

    #[test]
    fn test_ban_low_reputation() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, -90)).unwrap();

        coordinator
            .update_reputation(&[1], ReputationEventType::ChallengeLost)
            .unwrap();

        let worker = coordinator.get_worker(&[1]).unwrap();
        assert!(!worker.available); // Banned
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_verifiers_tee::{AttestationReport, TeeType};
    use proptest::prelude::*;

    fn make_report() -> AttestationReport {
        AttestationReport {
            tee_type: TeeType::Simulation,
            measurement: vec![1u8; 48],
            nonce: vec![2u8; 32],
            timestamp: current_timestamp(),
            signature: vec![3u8; 64],
            cert_chain: vec![vec![4u8; 16]],
        }
    }

    fn make_worker(id: Vec<u8>, reputation: i32, available: bool) -> WorkerInfo {
        let report = make_report();
        WorkerInfo {
            worker_id: id,
            tee_type: "sev-snp".to_string(),
            attestation: serde_json::to_vec(&report).unwrap(),
            capabilities: vec!["onnx".to_string()],
            reputation_score: reputation,
            available,
        }
    }

    fn base_reqs() -> JobRequirements {
        JobRequirements {
            tee_types: vec!["sev-snp".to_string()],
            capabilities: vec!["onnx".to_string()],
            min_reputation: 0,
        }
    }

    proptest! {
        /// Reputation score is always clamped to [-100, 1000] after any event.
        #[test]
        fn prop_reputation_clamped(initial in -100i32..=1000i32, events in proptest::collection::vec(0u8..5, 0..20)) {
            let mut coord = MeshCoordinator::new();
            coord.register_worker(make_worker(vec![1], initial, true)).unwrap();

            let event_types = [
                ReputationEventType::JobCompleted,
                ReputationEventType::JobFailed,
                ReputationEventType::ChallengeWon,
                ReputationEventType::ChallengeLost,
                ReputationEventType::Timeout,
            ];

            for e in events {
                let event = event_types[(e as usize) % event_types.len()].clone();
                let _ = coord.update_reputation(&[1], event);
            }

            if let Some(worker) = coord.get_worker(&[1]) {
                prop_assert!(worker.reputation_score >= -100);
                prop_assert!(worker.reputation_score <= 1000);
            }
        }

        /// Best worker (highest reputation) is always selected when multiple eligible workers exist.
        #[test]
        fn prop_best_worker_selected(
            rep_a in 50i32..=1000,
            rep_b in 50i32..=1000,
        ) {
            let mut coord = MeshCoordinator::new();
            coord.register_worker(make_worker(vec![1], rep_a, true)).unwrap();
            coord.register_worker(make_worker(vec![2], rep_b, true)).unwrap();

            let assigned = coord.assign_job(vec![42], &base_reqs()).unwrap();

            // Should assign to the worker with highest reputation
            if rep_a >= rep_b {
                prop_assert_eq!(assigned, vec![1]);
            } else {
                prop_assert_eq!(assigned, vec![2]);
            }
        }

        /// After assign+complete, available_worker_count returns to initial.
        #[test]
        fn prop_assign_complete_restores_availability(n_workers in 1usize..=8) {
            let mut coord = MeshCoordinator::new();
            for i in 0..n_workers {
                coord.register_worker(make_worker(vec![i as u8], 100, true)).unwrap();
            }

            let initial_available = coord.available_worker_count();

            // Assign one job
            let job_id = vec![99u8];
            let _ = coord.assign_job(job_id.clone(), &base_reqs());
            // Complete it
            let _ = coord.complete_job(&job_id);

            prop_assert_eq!(coord.available_worker_count(), initial_available);
        }

        /// After assign+cancel, available_worker_count returns to initial.
        #[test]
        fn prop_assign_cancel_restores_availability(n_workers in 1usize..=8) {
            let mut coord = MeshCoordinator::new();
            for i in 0..n_workers {
                coord.register_worker(make_worker(vec![i as u8], 100, true)).unwrap();
            }

            let initial_available = coord.available_worker_count();

            let job_id = vec![88u8];
            let _ = coord.assign_job(job_id.clone(), &base_reqs());
            let _ = coord.cancel_job(&job_id);

            prop_assert_eq!(coord.available_worker_count(), initial_available);
        }

        /// Duplicate job ID is always rejected.
        #[test]
        fn prop_duplicate_job_rejected(job_id in proptest::collection::vec(any::<u8>(), 1..=16)) {
            let mut coord = MeshCoordinator::new();
            coord.register_worker(make_worker(vec![1], 100, true)).unwrap();
            coord.register_worker(make_worker(vec![2], 50, true)).unwrap();

            let _ = coord.assign_job(job_id.clone(), &base_reqs());
            // Second assignment of same job must fail
            let result = coord.assign_job(job_id, &base_reqs());
            prop_assert!(result.is_err());
        }

        /// Worker with reputation <= -100 gets banned (unavailable).
        #[test]
        fn prop_ban_threshold(initial in -99i32..=0i32) {
            // Apply enough ChallengeLost events to push below -100
            let mut coord = MeshCoordinator::new();
            coord.register_worker(make_worker(vec![1], initial, true)).unwrap();

            // ChallengeLost = -50; two events pushes any score in [-99, 0] below -100
            for _ in 0..3 {
                let _ = coord.update_reputation(&[1], ReputationEventType::ChallengeLost);
            }

            if let Some(w) = coord.get_worker(&[1]) {
                if w.reputation_score <= -100 {
                    prop_assert!(!w.available, "banned worker must be unavailable");
                }
            }
        }
    }
}
