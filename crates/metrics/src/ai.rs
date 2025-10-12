use once_cell::sync::Lazy;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};

pub struct AiMetrics {
    pub jobs_completed: IntCounter,
    pub challenge_rate: Histogram,
    pub reputation_score: Histogram,
}

impl AiMetrics {
    fn new() -> Self {
        AiMetrics {
            jobs_completed: register_int_counter!(
                "aether_ai_jobs_completed",
                "Number of AI jobs settled"
            )
            .expect("register jobs_completed"),
            challenge_rate: register_histogram!(
                "aether_ai_challenge_rate",
                "Challenge rate for VCR proofs"
            )
            .expect("register challenge_rate"),
            reputation_score: register_histogram!(
                "aether_ai_reputation_score",
                "Distribution of provider reputation scores"
            )
            .expect("register reputation"),
        }
    }
}

pub static AI_METRICS: Lazy<AiMetrics> = Lazy::new(AiMetrics::new);
