use crate::monitoring::RouterMetrics;
use crate::scoring::{score_provider, ScoreWeights};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub job_id: String,
    pub required_capabilities: Vec<String>,
    pub min_reputation: i32,
    pub max_latency_ms: u64,
    pub max_price_per_unit: u64,
}

impl Default for JobRequest {
    fn default() -> Self {
        Self {
            job_id: "job-default".to_string(),
            required_capabilities: vec![],
            min_reputation: 0,
            max_latency_ms: 2_000,
            max_price_per_unit: 100_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCandidate {
    pub provider_id: String,
    pub capabilities: Vec<String>,
    pub reputation_score: i32,
    pub avg_latency_ms: u64,
    pub price_per_unit: u64,
    pub available: bool,
    pub active_jobs: u32,
    pub max_concurrent_jobs: u32,
}

impl Default for ProviderCandidate {
    fn default() -> Self {
        Self {
            provider_id: "provider-default".to_string(),
            capabilities: vec![],
            reputation_score: 50,
            avg_latency_ms: 200,
            price_per_unit: 1_000,
            available: true,
            active_jobs: 0,
            max_concurrent_jobs: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub job_id: String,
    pub provider_id: String,
    pub score: f64,
}

/// Route a job to the best-scoring provider.
pub fn route_job(job: &JobRequest, providers: &[ProviderCandidate]) -> Option<RoutingDecision> {
    route_job_with_metrics(job, providers, &mut RouterMetrics::new(128))
}

/// Same as `route_job`, with metrics capture for observability.
pub fn route_job_with_metrics(
    job: &JobRequest,
    providers: &[ProviderCandidate],
    metrics: &mut RouterMetrics,
) -> Option<RoutingDecision> {
    let mut ranked: Vec<(&ProviderCandidate, f64)> = providers
        .iter()
        .filter_map(|provider| {
            score_provider(job, provider, ScoreWeights::default()).map(|score| (provider, score))
        })
        .collect();

    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (provider, score) = ranked.first()?;

    metrics.record(job.job_id.clone(), provider.provider_id.clone(), *score);

    Some(RoutingDecision {
        job_id: job.job_id.clone(),
        provider_id: provider.provider_id.clone(),
        score: *score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::RouterMetrics;

    #[test]
    fn routes_to_best_provider() {
        let job = JobRequest {
            job_id: "job-123".to_string(),
            required_capabilities: vec!["onnx".to_string()],
            min_reputation: 10,
            max_latency_ms: 1_000,
            max_price_per_unit: 5_000,
        };

        let providers = vec![
            ProviderCandidate {
                provider_id: "slow".to_string(),
                capabilities: vec!["onnx".to_string()],
                reputation_score: 90,
                avg_latency_ms: 900,
                price_per_unit: 4_000,
                ..ProviderCandidate::default()
            },
            ProviderCandidate {
                provider_id: "best".to_string(),
                capabilities: vec!["onnx".to_string()],
                reputation_score: 95,
                avg_latency_ms: 100,
                price_per_unit: 2_000,
                ..ProviderCandidate::default()
            },
        ];

        let mut metrics = RouterMetrics::new(32);
        let decision = route_job_with_metrics(&job, &providers, &mut metrics).unwrap();

        assert_eq!(decision.provider_id, "best");
        assert_eq!(metrics.routed_jobs(), 1);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn eligible_provider(id: String, rep: i32) -> ProviderCandidate {
        ProviderCandidate {
            provider_id: id,
            capabilities: vec![],
            reputation_score: rep,
            avg_latency_ms: 200,
            price_per_unit: 1_000,
            available: true,
            active_jobs: 0,
            max_concurrent_jobs: 10,
        }
    }

    proptest! {
        /// route_job returns None when providers list is empty.
        #[test]
        fn prop_empty_providers_returns_none(_seed in any::<u8>()) {
            let job = JobRequest::default();
            prop_assert!(route_job(&job, &[]).is_none());
        }

        /// route_job result job_id matches the request job_id.
        #[test]
        fn prop_decision_job_id_matches(job_id in "[a-z0-9]{1,16}") {
            let job = JobRequest { job_id: job_id.clone(), ..JobRequest::default() };
            let providers = vec![eligible_provider("p1".to_string(), 50)];
            if let Some(decision) = route_job(&job, &providers) {
                prop_assert_eq!(decision.job_id, job_id);
            }
        }

        /// route_job decision score is in [0.0, 1.0].
        #[test]
        fn prop_decision_score_in_unit_interval(rep in 0i32..=100) {
            let job = JobRequest::default();
            let providers = vec![eligible_provider("p1".to_string(), rep)];
            if let Some(decision) = route_job(&job, &providers) {
                prop_assert!(decision.score >= 0.0);
                prop_assert!(decision.score <= 1.0);
            }
        }

        /// route_job selects the provider with highest reputation when all else equal.
        #[test]
        fn prop_highest_rep_wins(
            rep_a in 0i32..=50,
            rep_b in 51i32..=100,
        ) {
            let job = JobRequest::default();
            let providers = vec![
                eligible_provider("a".to_string(), rep_a),
                eligible_provider("b".to_string(), rep_b),
            ];
            let decision = route_job(&job, &providers).unwrap();
            prop_assert_eq!(decision.provider_id, "b");
        }

        /// All unavailable providers yields None.
        #[test]
        fn prop_all_unavailable_returns_none(n in 1usize..=5) {
            let job = JobRequest::default();
            let providers: Vec<_> = (0..n)
                .map(|i| ProviderCandidate {
                    provider_id: format!("p{i}"),
                    available: false,
                    ..ProviderCandidate::default()
                })
                .collect();
            prop_assert!(route_job(&job, &providers).is_none());
        }
    }
}
