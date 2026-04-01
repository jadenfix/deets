use std::collections::HashSet;

use aether_types::{Address, Slot, H256};
use serde::{Deserialize, Serialize};

use crate::ewma::Ewma;

const SUCCESS_WEIGHT: f64 = 0.4;
const LATENCY_WEIGHT: f64 = 0.3;
const UPTIME_WEIGHT: f64 = 0.2;
const DISPUTE_LOSS_PENALTY: f64 = 15.0;
const DISPUTE_WIN_BONUS: f64 = 2.0;
const MAX_SCORE: f64 = 100.0;
const MAX_LATENCY_MS: f64 = 30_000.0;
const SCORE_MIN: f64 = 0.0;
const ALPHA: f64 = 0.95;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HardwareTier {
    Standard,
    Premium,
    Dedicated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderReputation {
    pub address: Address,
    pub score: f64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub disputes_won: u32,
    pub disputes_lost: u32,
    pub last_active_slot: Slot,
    pub hardware_tier: HardwareTier,
    pub supported_models: HashSet<H256>,
    latency_ewma: Ewma,
    uptime_ewma: Ewma,
}

impl ProviderReputation {
    pub fn new(address: Address, tier: HardwareTier) -> Self {
        ProviderReputation {
            address,
            score: 50.0,
            jobs_completed: 0,
            jobs_failed: 0,
            disputes_won: 0,
            disputes_lost: 0,
            last_active_slot: 0,
            hardware_tier: tier,
            supported_models: HashSet::new(),
            latency_ewma: Ewma::new(ALPHA),
            uptime_ewma: Ewma::new(ALPHA),
        }
    }

    pub fn add_model(&mut self, model: H256) {
        self.supported_models.insert(model);
    }

    pub fn record_job_success(&mut self, latency_ms: f64, uptime_ratio: f64, slot: Slot) {
        self.jobs_completed += 1;
        self.latency_ewma.update(latency_ms);
        self.uptime_ewma.update(uptime_ratio);
        self.last_active_slot = slot;
        self.recompute_score();
    }

    pub fn record_job_failure(&mut self, slot: Slot) {
        self.jobs_failed += 1;
        self.score *= 0.9;
        self.last_active_slot = slot;
        self.score = self.score.clamp(SCORE_MIN, MAX_SCORE);
    }

    pub fn record_dispute(&mut self, won: bool) {
        if won {
            self.disputes_won += 1;
        } else {
            self.disputes_lost += 1;
        }
        self.recompute_score();
    }

    pub fn uptime(&self) -> f64 {
        self.uptime_ewma.value()
    }

    pub fn avg_latency(&self) -> f64 {
        self.latency_ewma.value()
    }

    fn recompute_score(&mut self) {
        let total_jobs = self.jobs_completed + self.jobs_failed;
        let success_rate = if total_jobs == 0 {
            0.0
        } else {
            self.jobs_completed as f64 / total_jobs as f64
        };

        let latency_score = if self.latency_ewma.initialized() {
            1.0 - (self.latency_ewma.value() / MAX_LATENCY_MS).min(1.0)
        } else {
            1.0
        };

        let uptime_score = self.uptime_ewma.value().clamp(0.0, 1.0);

        let mut score = SUCCESS_WEIGHT * success_rate * MAX_SCORE
            + LATENCY_WEIGHT * latency_score * MAX_SCORE
            + UPTIME_WEIGHT * uptime_score * MAX_SCORE;

        score -= self.disputes_lost as f64 * DISPUTE_LOSS_PENALTY;
        score += self.disputes_won as f64 * DISPUTE_WIN_BONUS;

        self.score = score.clamp(SCORE_MIN, MAX_SCORE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::H256;

    fn test_addr() -> Address {
        Address::from_slice(&[1u8; 20]).unwrap()
    }

    #[test]
    fn updates_score_on_success() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        rep.add_model(H256::zero());
        rep.record_job_success(100.0, 0.99, 10);
        assert!(rep.score > 50.0);
    }

    #[test]
    fn record_job_failure_reduces_score() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        let initial = rep.score;
        rep.record_job_failure(1);
        assert!(rep.score < initial);
        assert_eq!(rep.jobs_failed, 1);
        assert!(rep.score >= SCORE_MIN);
    }

    #[test]
    fn record_dispute_loss_penalty_not_doubled() {
        // This was the bug: record_dispute subtracted penalty immediately,
        // and recompute_score subtracted it again from counters.
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        // Record a success first so recompute_score has data
        rep.record_job_success(100.0, 0.99, 1);
        let score_after_success = rep.score;

        // Lose a dispute
        rep.record_dispute(false);
        let score_after_dispute = rep.score;
        assert!(score_after_dispute < score_after_success);

        // Record another success — score should NOT drop further due to dispute
        // (the dispute penalty should already be baked into the score)
        rep.record_job_success(100.0, 0.99, 2);
        let score_after_second_success = rep.score;
        // With 2 successes and 1 dispute loss, score should be consistent
        // and NOT lower than after the dispute (success should help, not hurt)
        assert!(
            score_after_second_success >= score_after_dispute,
            "score dropped after success: {} < {} (double-penalty bug)",
            score_after_second_success,
            score_after_dispute
        );
    }

    #[test]
    fn record_dispute_win_bonus() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        rep.record_job_success(100.0, 0.99, 1);
        let before = rep.score;
        rep.record_dispute(true);
        assert!(rep.score > before || rep.score == MAX_SCORE);
        assert_eq!(rep.disputes_won, 1);
    }

    #[test]
    fn score_clamped_to_min() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        // Lose many disputes
        for _ in 0..20 {
            rep.record_dispute(false);
        }
        assert_eq!(rep.score, SCORE_MIN);
    }

    #[test]
    fn score_clamped_to_max() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        for _ in 0..100 {
            rep.record_dispute(true);
        }
        assert_eq!(rep.score, MAX_SCORE);
    }

    #[test]
    fn uptime_and_avg_latency() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        assert_eq!(rep.uptime(), 0.0);
        assert_eq!(rep.avg_latency(), 0.0);

        rep.record_job_success(200.0, 0.95, 1);
        assert!((rep.avg_latency() - 200.0).abs() < 0.01);
        assert!((rep.uptime() - 0.95).abs() < 0.01);
    }

    #[test]
    fn add_model() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        let m1 = H256([1u8; 32]);
        let m2 = H256([2u8; 32]);
        rep.add_model(m1);
        rep.add_model(m2);
        rep.add_model(m1); // duplicate
        assert_eq!(rep.supported_models.len(), 2);
    }

    #[test]
    fn mixed_success_failure_dispute_consistency() {
        let mut rep = ProviderReputation::new(test_addr(), HardwareTier::Standard);
        rep.record_job_success(100.0, 0.99, 1);
        rep.record_job_failure(2);
        rep.record_dispute(false);
        rep.record_job_success(150.0, 0.95, 3);

        // Score should be deterministic from current state
        let score1 = rep.score;
        rep.recompute_score();
        let score2 = rep.score;
        assert!(
            (score1 - score2).abs() < f64::EPSILON,
            "recompute_score is not idempotent: {} vs {}",
            score1,
            score2
        );
    }
}
