// ============================================================================
// AETHER METRICS - Observability & Instrumentation
// ============================================================================
// PURPOSE: Prometheus metrics for monitoring node health & performance
//
// KEY METRICS:
// - Consensus: slot_time, fork_rate, finality_latency
// - Mempool: depth, admission_rate, eviction_rate
// - Runtime: tx_execution_time, parallel_speedup, gas_per_tx
// - P2P: peer_count, message_rate, bandwidth
// - AI: jobs_completed, vcr_challenge_rate, provider_reputation
//
// USAGE:
//   METRICS.slot_finalized.inc();
//   METRICS.tx_execution_time.observe(duration_ms);
// ============================================================================

pub mod ai;
pub mod consensus;
pub mod da;
pub mod exporter;
pub mod networking;
pub mod p2p;
pub mod runtime;

pub use ai::AI_METRICS;
pub use consensus::CONSENSUS_METRICS;
pub use da::DA_METRICS;
pub use networking::NET_METRICS;
pub use p2p::P2P_METRICS;
pub use runtime::RUNTIME_METRICS;
