// ============================================================================
// AETHER STAKING PROGRAM - SWR Token Staking
// ============================================================================
// PURPOSE: Manage validator staking, delegation, and rewards
//
// OPERATIONS:
// - register_validator: Create new validator
// - delegate: Delegate SWR to validator
// - unbond: Start unbonding period (7 days)
// - complete_unbond: Claim unbonded tokens
// - distribute_rewards: Epoch reward distribution
// - slash: Penalize misbehavior
//
// ECONOMICS:
// - Min stake: 100 SWR
// - Unbonding: 7 days (100,800 slots)
// - Rewards: 5% APY
// - Commission: 0-100% (set by validator)
// - Slashing: 5% for double-sign, 0.001%/slot for downtime
//
// STATE:
// - Validators: address, stake, commission, status
// - Delegations: delegator -> validator -> amount
// - Unbonding queue: address -> amount -> completion_slot
// - Reward pool: accumulated rewards
// ============================================================================

pub mod state;

pub use state::{StakingState, Validator, Delegation, Unbonding};
