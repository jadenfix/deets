// ============================================================================
// AETHER REPUTATION PROGRAM - Provider Quality Tracking
// ============================================================================
// PURPOSE: Track AI provider performance for intelligent job routing
//
// ALGORITHM: EWMA (Exponentially Weighted Moving Average) scoring
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    REPUTATION ORACLE                              │
// ├──────────────────────────────────────────────────────────────────┤
// │  Job Completion  →  Performance Metrics  →  Score Update          │
// │         ↓                                      ↓                  │
// │  Dispute Resolution  →  Penalty/Reward  →  Reputation Adjust     │
// │         ↓                                      ↓                  │
// │  Router Query  →  Top Providers  →  Job Assignment               │
// └──────────────────────────────────────────────────────────────────┘
//
// PROVIDER STATE:
// ```
// struct ProviderReputation:
//     address: Address
//     score: f64  // 0.0 to 100.0
//     jobs_completed: u64
//     jobs_failed: u64
//     total_latency_ms: u64
//     avg_latency_ms: f64  // EWMA
//     disputes_won: u32
//     disputes_lost: u32
//     uptime_ratio: f64  // EWMA
//     last_active_slot: u64
//     hardware_tier: HardwareTier
//     supported_models: Vec<H256>
// ```
//
// SCORING:
// ```
// fn calculate_score(provider) -> f64:
//     success_rate = provider.jobs_completed / (provider.jobs_completed + provider.jobs_failed)
//     latency_score = 100.0 * (1.0 - provider.avg_latency_ms / MAX_LATENCY)
//     dispute_penalty = provider.disputes_lost * DISPUTE_PENALTY
//     uptime_score = provider.uptime_ratio * 100.0
//     
//     score = 0.4 * success_rate * 100.0
//           + 0.3 * latency_score
//           + 0.2 * uptime_score
//           - dispute_penalty
//     
//     return clamp(score, 0.0, 100.0)
// ```
//
// EWMA UPDATE:
// ```
// const ALPHA: f64 = 0.95;  // Decay factor
//
// fn update_on_job_completion(provider, job):
//     provider.jobs_completed += 1
//     
//     // Update latency EWMA
//     job_latency = job.completion_slot - job.accepted_slot
//     provider.avg_latency_ms = ALPHA * provider.avg_latency_ms + (1.0 - ALPHA) * job_latency
//     
//     // Recalculate score
//     provider.score = calculate_score(provider)
//     
//     provider.last_active_slot = current_slot
//
// fn update_on_job_failure(provider, job):
//     provider.jobs_failed += 1
//     
//     // Heavy penalty for failures
//     provider.score = provider.score * 0.9
//     
//     provider.last_active_slot = current_slot
//
// fn update_on_dispute(provider, won: bool):
//     if won:
//         provider.disputes_won += 1
//         provider.score += DISPUTE_WIN_BONUS
//     else:
//         provider.disputes_lost += 1
//         provider.score -= DISPUTE_LOSS_PENALTY
//     
//     provider.score = clamp(provider.score, 0.0, 100.0)
// ```
//
// ROUTER QUERIES:
// ```
// fn get_top_providers(model_hash, hardware_tier, min_score, limit) -> Vec<Address>:
//     candidates = []
//     
//     for provider in all_providers:
//         if provider.score < min_score:
//             continue
//         if !provider.supported_models.contains(model_hash):
//             continue
//         if provider.hardware_tier < hardware_tier:
//             continue
//         if current_slot - provider.last_active_slot > STALENESS_THRESHOLD:
//             continue  // Provider inactive
//         
//         candidates.push(provider)
//     
//     // Sort by score descending
//     candidates.sort_by(|a, b| b.score.cmp(a.score))
//     
//     return candidates[0..min(limit, candidates.len())]
//
// fn get_provider_details(address) -> Option<ProviderReputation>:
//     return providers.get(address)
// ```
//
// PARAMETERS:
// - ALPHA: 0.95 (EWMA decay)
// - DISPUTE_PENALTY: 10.0 points per loss
// - DISPUTE_WIN_BONUS: 2.0 points per win
// - DISPUTE_LOSS_PENALTY: 15.0 points per loss
// - STALENESS_THRESHOLD: 43200 slots (6 hours)
// - MAX_LATENCY: 30000 ms (for normalization)
//
// OUTPUTS:
// - Provider rankings → Router job assignment
// - Reputation scores → User provider selection
// - Historical data → Analytics & monitoring
// ============================================================================

pub mod scoring;
pub mod ewma;
pub mod queries;

pub use scoring::ProviderReputation;

