// ============================================================================
// AETHER STAKING PROGRAM - SWR Token Staking & Validator Management
// ============================================================================
// PURPOSE: Secure the network via proof-of-stake, distribute rewards, slash
//
// TOKEN: SWR (Staking/Governance token)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    STAKING PROGRAM                                │
// ├──────────────────────────────────────────────────────────────────┤
// │  User Stakes SWR  →  Bond Transaction  →  Validator Set Update   │
// │         ↓                                      ↓                  │
// │  Consensus Uses Stake  →  VRF Election  →  Block Production      │
// │         ↓                                      ↓                  │
// │  Epoch End  →  Rewards Calculation  →  Distribute to Stakers     │
// │         ↓                                      ↓                  │
// │  Slashing Event  →  Evidence Verification  →  Burn Stake         │
// └──────────────────────────────────────────────────────────────────┘
//
// STATE:
// ```
// struct ValidatorState:
//     pubkey: PublicKey
//     stake: u128
//     commission: u16  // Basis points (e.g., 1000 = 10%)
//     delegated_stake: u128
//     delegators: Vec<(Address, u128)>
//     rewards_earned: u128
//     last_reward_epoch: u64
//     slashed: bool
//     unbonding: Vec<UnbondingEntry>
//
// struct UnbondingEntry:
//     amount: u128
//     completion_slot: u64
// ```
//
// OPERATIONS:
// ```
// fn stake(validator_pubkey, amount):
//     // Transfer SWR to staking account
//     transfer_swr(caller, STAKING_ACCOUNT, amount)
//     
//     // Update validator state
//     validator = get_validator(validator_pubkey)
//     if caller == validator_pubkey:
//         validator.stake += amount
//     else:
//         validator.delegated_stake += amount
//         validator.delegators.push((caller, amount))
//     
//     // Update total stake
//     total_stake += amount
//
// fn unstake(validator_pubkey, amount):
//     validator = get_validator(validator_pubkey)
//     
//     // Start unbonding period
//     completion_slot = current_slot + UNBONDING_DELAY
//     validator.unbonding.push(UnbondingEntry {
//         amount: amount,
//         completion_slot: completion_slot
//     })
//     
//     // Update stake immediately (for consensus weight)
//     if caller == validator_pubkey:
//         validator.stake -= amount
//     else:
//         validator.delegated_stake -= amount
//         remove_delegator(validator, caller, amount)
//     
//     total_stake -= amount
//
// fn claim_unbonded():
//     validator = get_validator(caller_pubkey)
//     
//     claimable = []
//     for entry in validator.unbonding:
//         if entry.completion_slot <= current_slot:
//             claimable.push(entry)
//     
//     total_claimable = sum(claimable.map(|e| e.amount))
//     
//     // Transfer SWR back
//     transfer_swr(STAKING_ACCOUNT, caller, total_claimable)
//     
//     // Remove from unbonding queue
//     validator.unbonding.retain(|e| e.completion_slot > current_slot)
//
// fn distribute_rewards(epoch):
//     total_reward = calculate_epoch_reward(epoch)
//     
//     for validator in validators:
//         if validator.slashed:
//             continue
//         
//         // Calculate validator share based on stake
//         validator_total = validator.stake + validator.delegated_stake
//         validator_reward = total_reward * validator_total / total_stake
//         
//         // Commission
//         commission_amount = validator_reward * validator.commission / 10000
//         validator.rewards_earned += commission_amount
//         
//         // Distribute to delegators
//         delegator_reward = validator_reward - commission_amount
//         for (delegator, delegated_amount) in validator.delegators:
//             delegator_share = delegator_reward * delegated_amount / validator.delegated_stake
//             credit_rewards(delegator, delegator_share)
//
// fn slash(validator_pubkey, evidence):
//     validator = get_validator(validator_pubkey)
//     
//     match evidence:
//         DoubleSign(proof):
//             // Slash 5% of stake
//             slash_amount = validator.stake * SLASH_DOUBLE / 100
//             burn_swr(slash_amount)
//             validator.stake -= slash_amount
//             validator.slashed = true
//         
//         Downtime(proof):
//             // Gradual leak (per slot missed)
//             leak_amount = validator.stake * LEAK_DOWNTIME
//             burn_swr(leak_amount)
//             validator.stake -= leak_amount
// ```
//
// REWARDS MODEL:
// - Annual inflation: 8% (decreasing over time)
// - Distributed proportionally to stake
// - Commission: validator sets rate (e.g., 10%)
// - Compound every epoch (auto-restake option)
//
// SLASHING:
// 1. Double-sign: 5% stake burn + jailing (can't validate)
// 2. Downtime: Gradual leak (0.001% per missed slot)
//
// PARAMETERS (from genesis.toml):
// - UNBONDING_DELAY: 172800 slots (24 hours)
// - SLASH_DOUBLE: 5%
// - LEAK_DOWNTIME: 0.00001 per slot
// - MAX_COMMISSION: 20%
//
// OUTPUTS:
// - Validator set → Consensus (for VRF election)
// - Stake table → Vote aggregation (for quorum)
// - Reward distribution → SWR token balances
// - Slashing events → Fraud proof verification
// ============================================================================

pub mod validator;
pub mod delegation;
pub mod rewards;
pub mod slashing;

pub use validator::ValidatorState;

