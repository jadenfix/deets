use once_cell::sync::Lazy;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};

pub struct ConsensusMetrics {
    pub slots_finalized: IntCounter,
    pub fork_events: IntCounter,
    pub finality_latency_ms: Histogram,
}

impl ConsensusMetrics {
    fn new() -> Self {
        ConsensusMetrics {
            slots_finalized: register_int_counter!(
                "aether_consensus_slots_finalized",
                "Number of slots finalized"
            )
            .expect("register slots_finalized"),
            fork_events: register_int_counter!(
                "aether_consensus_fork_events",
                "Observed fork events"
            )
            .expect("register fork_events"),
            finality_latency_ms: register_histogram!(
                "aether_consensus_finality_latency_ms",
                "Latency from block production to finality"
            )
            .expect("register finality latency"),
        }
    }
}

pub static CONSENSUS_METRICS: Lazy<ConsensusMetrics> = Lazy::new(ConsensusMetrics::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_counters() {
        CONSENSUS_METRICS.slots_finalized.inc();
        CONSENSUS_METRICS.fork_events.inc_by(2);
        CONSENSUS_METRICS.finality_latency_ms.observe(42.0);
    }
}
