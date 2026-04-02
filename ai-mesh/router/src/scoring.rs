use crate::routing::{JobRequest, ProviderCandidate};

#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights {
    pub reputation: f64,
    pub latency: f64,
    pub price: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            reputation: 0.5,
            latency: 0.3,
            price: 0.2,
        }
    }
}

pub fn score_provider(
    job: &JobRequest,
    provider: &ProviderCandidate,
    weights: ScoreWeights,
) -> Option<f64> {
    if !provider.available {
        return None;
    }
    if provider.reputation_score < job.min_reputation {
        return None;
    }
    if provider.avg_latency_ms > job.max_latency_ms {
        return None;
    }
    if provider.price_per_unit > job.max_price_per_unit {
        return None;
    }

    if !job
        .required_capabilities
        .iter()
        .all(|cap| provider.capabilities.contains(cap))
    {
        return None;
    }

    let normalized_rep = (provider.reputation_score as f64 / 100.0).clamp(0.0, 1.0);
    let latency_ratio = provider.avg_latency_ms as f64 / job.max_latency_ms as f64;
    let normalized_latency = (1.0 - latency_ratio).clamp(0.0, 1.0);
    let price_ratio = provider.price_per_unit as f64 / job.max_price_per_unit as f64;
    let normalized_price = (1.0 - price_ratio).clamp(0.0, 1.0);

    let load_ratio = if provider.max_concurrent_jobs == 0 {
        1.0
    } else {
        provider.active_jobs as f64 / provider.max_concurrent_jobs as f64
    };
    let load_penalty = (1.0 - 0.5 * load_ratio).clamp(0.0, 1.0);

    let weighted = normalized_rep * weights.reputation
        + normalized_latency * weights.latency
        + normalized_price * weights.price;
    Some(weighted * load_penalty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{JobRequest, ProviderCandidate};

    #[test]
    fn higher_reputation_scores_better() {
        let job = JobRequest::default();
        let low = ProviderCandidate {
            provider_id: "low".to_string(),
            reputation_score: 20,
            ..ProviderCandidate::default()
        };
        let high = ProviderCandidate {
            provider_id: "high".to_string(),
            reputation_score: 90,
            ..ProviderCandidate::default()
        };

        let low_score = score_provider(&job, &low, ScoreWeights::default()).unwrap();
        let high_score = score_provider(&job, &high, ScoreWeights::default()).unwrap();
        assert!(high_score > low_score);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::routing::{JobRequest, ProviderCandidate};
    use proptest::prelude::*;

    proptest! {
        /// score_provider always returns a value in [0.0, 1.0] for eligible providers.
        #[test]
        fn prop_score_in_unit_interval(
            rep in 0i32..=100,
            latency_ms in 1u64..=1_000,
            price in 1u64..=50_000,
            active in 0u32..=5,
            max_concurrent in 1u32..=10,
        ) {
            let job = JobRequest::default();
            let provider = ProviderCandidate {
                provider_id: "p".to_string(),
                capabilities: vec![],
                reputation_score: rep,
                avg_latency_ms: latency_ms,
                price_per_unit: price,
                available: true,
                active_jobs: active.min(max_concurrent),
                max_concurrent_jobs: max_concurrent,
            };

            if let Some(score) = score_provider(&job, &provider, ScoreWeights::default()) {
                prop_assert!(score >= 0.0, "score must be non-negative: {score}");
                prop_assert!(score <= 1.0, "score must be <= 1.0: {score}");
            }
        }

        /// Unavailable provider always returns None.
        #[test]
        fn prop_unavailable_provider_scores_none(rep in 0i32..=100) {
            let job = JobRequest::default();
            let provider = ProviderCandidate {
                available: false,
                reputation_score: rep,
                ..ProviderCandidate::default()
            };
            prop_assert!(score_provider(&job, &provider, ScoreWeights::default()).is_none());
        }

        /// Provider exceeding max_latency_ms returns None.
        #[test]
        fn prop_over_latency_scores_none(excess in 1u64..=10_000) {
            let job = JobRequest::default(); // max_latency_ms = 2000
            let provider = ProviderCandidate {
                avg_latency_ms: job.max_latency_ms + excess,
                ..ProviderCandidate::default()
            };
            prop_assert!(score_provider(&job, &provider, ScoreWeights::default()).is_none());
        }

        /// Provider exceeding max_price_per_unit returns None.
        #[test]
        fn prop_over_price_scores_none(excess in 1u64..=100_000) {
            let job = JobRequest::default(); // max_price_per_unit = 100_000
            let provider = ProviderCandidate {
                price_per_unit: job.max_price_per_unit + excess,
                ..ProviderCandidate::default()
            };
            prop_assert!(score_provider(&job, &provider, ScoreWeights::default()).is_none());
        }

        /// Higher reputation (same latency/price) yields strictly higher score.
        #[test]
        fn prop_higher_rep_scores_better(
            rep_low in 0i32..=49,
            rep_high in 50i32..=100,
        ) {
            let job = JobRequest::default();
            let low = ProviderCandidate { reputation_score: rep_low, ..ProviderCandidate::default() };
            let high = ProviderCandidate { reputation_score: rep_high, ..ProviderCandidate::default() };

            let score_low = score_provider(&job, &low, ScoreWeights::default()).unwrap();
            let score_high = score_provider(&job, &high, ScoreWeights::default()).unwrap();
            prop_assert!(score_high >= score_low);
        }
    }
}
