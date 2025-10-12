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

use anyhow::{Result, bail};
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
        MeshCoordinator {
            workers: HashMap::new(),
            assignments: HashMap::new(),
            reputation: HashMap::new(),
        }
    }

    /// Register a new worker
    pub fn register_worker(&mut self, worker: WorkerInfo) -> Result<()> {
        // Verify TEE attestation
        if worker.attestation.is_empty() {
            bail!("missing attestation");
        }

        // TODO: Verify attestation with TEE verifier

        self.workers.insert(worker.worker_id.clone(), worker);

        Ok(())
    }

    /// Find best worker for a job
    pub fn assign_job(&mut self, job_id: Vec<u8>, requirements: &JobRequirements) -> Result<Vec<u8>> {
        // Find eligible workers
        let mut candidates: Vec<&WorkerInfo> = self.workers
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

        self.assignments.insert(job_id, assignment.clone());

        Ok(best_worker.worker_id.clone())
    }

    /// Update worker reputation
    pub fn update_reputation(
        &mut self,
        worker_id: &[u8],
        event_type: ReputationEventType,
    ) -> Result<()> {
        let worker = self.workers.get_mut(worker_id)
            .ok_or_else(|| anyhow::anyhow!("worker not found"))?;

        let score_change = match event_type {
            ReputationEventType::JobCompleted => 10,
            ReputationEventType::JobFailed => -20,
            ReputationEventType::ChallengeWon => 5,
            ReputationEventType::ChallengeLost => -50,
            ReputationEventType::Timeout => -30,
        };

        worker.reputation_score += score_change;

        // Record event
        let event = ReputationEvent {
            timestamp: current_timestamp(),
            event_type: event_type.clone(),
            score_change,
        };

        self.reputation
            .entry(worker_id.to_vec())
            .or_insert_with(Vec::new)
            .push(event);

        // Ban worker if reputation too low
        if worker.reputation_score < -100 {
            worker.available = false;
            println!("Worker {:?} banned (low reputation)", hex::encode(worker_id));
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
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_worker(id: u8, reputation: i32) -> WorkerInfo {
        WorkerInfo {
            worker_id: vec![id],
            tee_type: "sev-snp".to_string(),
            attestation: vec![1, 2, 3],
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
        
        let assigned = coordinator.assign_job(vec![1, 2, 3], &requirements).unwrap();
        
        // Should assign to worker 1 (higher reputation)
        assert_eq!(assigned, vec![1]);
    }

    #[test]
    fn test_reputation_update() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, 0)).unwrap();
        
        coordinator.update_reputation(&[1], ReputationEventType::JobCompleted).unwrap();
        
        let worker = coordinator.get_worker(&[1]).unwrap();
        assert_eq!(worker.reputation_score, 10);
    }

    #[test]
    fn test_ban_low_reputation() {
        let mut coordinator = MeshCoordinator::new();
        coordinator.register_worker(test_worker(1, -90)).unwrap();
        
        coordinator.update_reputation(&[1], ReputationEventType::ChallengeLost).unwrap();
        
        let worker = coordinator.get_worker(&[1]).unwrap();
        assert!(!worker.available); // Banned
    }
}

