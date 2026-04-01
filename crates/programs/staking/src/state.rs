use aether_types::Address;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StakingError {
    #[error("insufficient stake: minimum is 100 SWR (100_000_000 base units), have {got} base units (min={min})")]
    InsufficientStake { min: u128, got: u128 },
    #[error("validator already exists: {0:?}")]
    ValidatorExists(Address),
    #[error("unauthorized: caller must match validator address")]
    Unauthorized,
    #[error("validator not found: {0:?}")]
    ValidatorNotFound(Address),
    #[error("validator is not active: {0:?}")]
    ValidatorInactive(Address),
    #[error("invalid commission rate: {0} (max 10000 bps)")]
    InvalidCommission(u16),
    #[error("invalid slash rate: {0} (max 10000 bps)")]
    InvalidSlashRate(u128),
    #[error("delegation not found")]
    DelegationNotFound,
    #[error("insufficient delegation: have {have}, requested {requested}")]
    InsufficientDelegation { have: u128, requested: u128 },
    #[error("arithmetic overflow in balance calculation")]
    Overflow,
}

/// Staking Program State
///
/// Manages SWR token staking for consensus security.
///
/// Features:
/// - Bond/unbond with unbonding period
/// - Delegation to validators
/// - Reward distribution
/// - Slashing for misbehavior
///
/// Economics:
/// - Min stake: 100 SWR
/// - Unbonding period: 7 days (100,800 slots)
/// - Reward rate: 5% APY
/// - Slash rate: 5% for double-sign, 0.001% per slot for downtime

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakingState {
    /// Total staked amount
    pub total_staked: u128,

    /// All validators
    pub validators: Vec<Validator>,

    /// All delegations
    pub delegations: Vec<Delegation>,

    /// Pending unbonds
    pub unbonding: Vec<Unbonding>,

    /// Reward pool
    pub reward_pool: u128,

    /// Current epoch
    pub current_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Validator {
    pub address: Address,
    pub staked_amount: u128,
    pub delegated_amount: u128,
    pub commission_rate: u16, // Basis points (e.g., 1000 = 10%)
    pub reward_address: Address,
    pub is_active: bool,
    pub jailed_until: Option<u64>, // Slot number
    pub slash_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delegation {
    pub delegator: Address,
    pub validator: Address,
    pub amount: u128,
    pub reward_debt: u128, // For reward calculation
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unbonding {
    pub address: Address,
    pub amount: u128,
    pub complete_slot: u64,
}

impl StakingState {
    pub fn new() -> Self {
        StakingState {
            total_staked: 0,
            validators: Vec::new(),
            delegations: Vec::new(),
            unbonding: Vec::new(),
            reward_pool: 0,
            current_epoch: 0,
        }
    }

    /// Register a new validator.
    ///
    /// `caller` must match `address` to prevent impersonation.
    pub fn register_validator(
        &mut self,
        caller: Address,
        address: Address,
        initial_stake: u128,
        commission_rate: u16,
        reward_address: Address,
    ) -> Result<(), StakingError> {
        // Authority check: only the validator itself can register
        if caller != address {
            return Err(StakingError::Unauthorized);
        }

        const MIN_STAKE: u128 = 100_000_000; // 100 SWR with 6 decimals
        if initial_stake < MIN_STAKE {
            return Err(StakingError::InsufficientStake {
                min: MIN_STAKE,
                got: initial_stake,
            });
        }

        if self.validators.iter().any(|v| v.address == address) {
            return Err(StakingError::ValidatorExists(address));
        }

        if commission_rate > 10000 {
            return Err(StakingError::InvalidCommission(commission_rate));
        }

        let validator = Validator {
            address,
            staked_amount: initial_stake,
            delegated_amount: 0,
            commission_rate,
            reward_address,
            is_active: true,
            jailed_until: None,
            slash_count: 0,
        };

        self.validators.push(validator);
        self.total_staked = self.total_staked.checked_add(initial_stake).ok_or(StakingError::Overflow)?;

        Ok(())
    }

    /// Delegate to a validator
    pub fn delegate(
        &mut self,
        caller: Address,
        delegator: Address,
        validator: Address,
        amount: u128,
    ) -> Result<(), StakingError> {
        if caller != delegator {
            return Err(StakingError::Unauthorized);
        }

        let validator_idx = self
            .validators
            .iter()
            .position(|v| v.address == validator)
            .ok_or(StakingError::ValidatorNotFound(validator))?;

        if !self.validators[validator_idx].is_active {
            return Err(StakingError::ValidatorInactive(validator));
        }

        // Create or update delegation
        if let Some(delegation) = self
            .delegations
            .iter_mut()
            .find(|d| d.delegator == delegator && d.validator == validator)
        {
            delegation.amount = delegation.amount.checked_add(amount).ok_or(StakingError::Overflow)?;
        } else {
            self.delegations.push(Delegation {
                delegator,
                validator,
                amount,
                reward_debt: 0,
            });
        }

        // Update validator
        self.validators[validator_idx].delegated_amount = self.validators[validator_idx].delegated_amount.checked_add(amount).ok_or(StakingError::Overflow)?;
        self.total_staked = self.total_staked.checked_add(amount).ok_or(StakingError::Overflow)?;

        Ok(())
    }

    /// Unbond stake (start unbonding period)
    pub fn unbond(
        &mut self,
        caller: Address,
        delegator: Address,
        validator: Address,
        amount: u128,
        current_slot: u64,
    ) -> Result<(), StakingError> {
        if caller != delegator {
            return Err(StakingError::Unauthorized);
        }

        let delegation = self
            .delegations
            .iter_mut()
            .find(|d| d.delegator == delegator && d.validator == validator)
            .ok_or(StakingError::DelegationNotFound)?;

        if amount > delegation.amount {
            return Err(StakingError::InsufficientDelegation {
                have: delegation.amount,
                requested: amount,
            });
        }

        // Update delegation
        delegation.amount = delegation.amount.checked_sub(amount).ok_or(StakingError::Overflow)?;

        // Remove if zero
        if delegation.amount == 0 {
            self.delegations
                .retain(|d| !(d.delegator == delegator && d.validator == validator));
        }

        // Update validator
        if let Some(v) = self.validators.iter_mut().find(|v| v.address == validator) {
            v.delegated_amount = v.delegated_amount.checked_sub(amount).ok_or(StakingError::Overflow)?;
        }

        // Add to unbonding queue (7 days = 100,800 slots at 500ms/slot)
        self.unbonding.push(Unbonding {
            address: delegator,
            amount,
            complete_slot: current_slot + 100_800,
        });

        self.total_staked = self.total_staked.checked_sub(amount).ok_or(StakingError::Overflow)?;

        Ok(())
    }

    /// Complete unbonding (transfer tokens back)
    pub fn complete_unbonding(&mut self, current_slot: u64) -> Vec<(Address, u128)> {
        let mut completed = Vec::new();

        self.unbonding.retain(|u| {
            if u.complete_slot <= current_slot {
                completed.push((u.address, u.amount));
                false
            } else {
                true
            }
        });

        completed
    }

    /// Slash a validator for misbehavior.
    ///
    /// NOTE: `slash_count` increments permanently and jails the validator at 3 slashes,
    /// but there is currently no `unjail()` mechanism. Once jailed, a validator cannot
    /// be reactivated. An unjail function (with a cooldown and/or governance vote)
    /// should be added before mainnet.
    pub fn slash(
        &mut self,
        validator: Address,
        slash_rate: u128, // Basis points (e.g., 500 = 5%)
    ) -> Result<u128, StakingError> {
        if slash_rate > 10000 {
            return Err(StakingError::InvalidSlashRate(slash_rate));
        }

        let validator_idx = self
            .validators
            .iter()
            .position(|v| v.address == validator)
            .ok_or(StakingError::ValidatorNotFound(validator))?;

        // Calculate slash amount
        let slash_amount = self.validators[validator_idx].staked_amount.saturating_mul(slash_rate) / 10000;

        // Apply slash
        self.validators[validator_idx].staked_amount = self.validators[validator_idx]
            .staked_amount
            .saturating_sub(slash_amount);
        self.validators[validator_idx].slash_count = self.validators[validator_idx].slash_count.saturating_add(1);

        // Slash each delegation entry so validator aggregate, unbonding, and
        // reward distribution all operate on the same post-slash balances.
        let mut delegated_slash = 0u128;
        for delegation in self
            .delegations
            .iter_mut()
            .filter(|delegation| delegation.validator == validator)
        {
            let slash = delegation.amount.saturating_mul(slash_rate) / 10000;
            let remaining = delegation.amount.saturating_sub(slash);
            delegated_slash = delegated_slash.saturating_add(delegation.amount.saturating_sub(remaining));
            delegation.amount = remaining;
        }
        self.delegations.retain(|delegation| delegation.amount > 0);
        self.validators[validator_idx].delegated_amount = self
            .delegations
            .iter()
            .filter(|delegation| delegation.validator == validator)
            .map(|delegation| delegation.amount)
            .sum();
        let total_slash = slash_amount + delegated_slash;
        self.total_staked = self.total_staked.saturating_sub(total_slash);

        // Jail validator if slashed too many times
        if self.validators[validator_idx].slash_count >= 3 {
            self.validators[validator_idx].is_active = false;
        }

        Ok(total_slash)
    }

    /// Distribute rewards proportionally to validators and their delegators.
    ///
    /// For each active validator:
    ///   1. Compute their share: epoch_rewards * (validator_stake + delegated) / total_staked
    ///   2. Validator takes commission (commission_rate bps) from that share
    ///   3. Remaining reward is distributed to delegators proportionally by delegation amount
    pub fn distribute_rewards(&mut self, epoch_rewards: u128) {
        if self.total_staked == 0 || epoch_rewards == 0 {
            return;
        }

        self.reward_pool = self.reward_pool.saturating_add(epoch_rewards);

        // Collect validator reward info first (to avoid borrow issues)
        let validator_infos: Vec<(Address, u128, u128, u16, bool)> = self
            .validators
            .iter()
            .map(|v| {
                (
                    v.address,
                    v.staked_amount,
                    v.delegated_amount,
                    v.commission_rate,
                    v.is_active,
                )
            })
            .collect();

        for (val_addr, val_stake, delegated_amount, commission_rate, is_active) in &validator_infos
        {
            if !is_active {
                continue;
            }

            let total_stake = val_stake.saturating_add(*delegated_amount);
            if total_stake == 0 {
                continue;
            }

            let validator_reward = (epoch_rewards * total_stake) / self.total_staked;
            let commission = (validator_reward * *commission_rate as u128) / 10000;
            let delegator_pool = validator_reward.saturating_sub(commission);

            // Credit commission to validator
            if let Some(v) = self.validators.iter_mut().find(|v| v.address == *val_addr) {
                v.staked_amount = v.staked_amount.saturating_add(commission);
            }

            // Distribute remaining rewards to delegators proportionally.
            // Track distributed amount to handle rounding remainder.
            if *delegated_amount > 0 && delegator_pool > 0 {
                let mut distributed = 0u128;
                let mut last_delegation_idx = None;
                for (idx, delegation) in self.delegations.iter_mut().enumerate() {
                    if delegation.validator == *val_addr && delegation.amount > 0 {
                        let delegator_share =
                            (delegator_pool * delegation.amount) / delegated_amount;
                        delegation.amount = delegation.amount.saturating_add(delegator_share);
                        distributed = distributed.saturating_add(delegator_share);
                        last_delegation_idx = Some(idx);
                    }
                }
                // Give rounding remainder to the last delegator to prevent reward leakage
                let remainder = delegator_pool.saturating_sub(distributed);
                if remainder > 0 {
                    if let Some(idx) = last_delegation_idx {
                        self.delegations[idx].amount = self.delegations[idx].amount.saturating_add(remainder);
                    }
                }
            }
        }
    }

    pub fn get_validator(&self, address: &Address) -> Option<&Validator> {
        self.validators.iter().find(|v| v.address == *address)
    }

    pub fn get_total_staked(&self) -> u128 {
        self.total_staked
    }

    pub fn active_validators(&self) -> Vec<&Validator> {
        self.validators.iter().filter(|v| v.is_active).collect()
    }
}

impl Default for StakingState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_address(n: u8) -> Address {
        Address::from_slice(&[n; 20]).unwrap()
    }

    #[test]
    fn test_register_validator() {
        let mut state = StakingState::new();

        let result = state.register_validator(
            test_address(1), // caller
            test_address(1), // address
            1_000_000_000,   // 1000 SWR
            1000,            // 10% commission
            test_address(2), // reward_address
        );

        assert!(result.is_ok());
        assert_eq!(state.validators.len(), 1);
        assert_eq!(state.total_staked, 1_000_000_000);
    }

    #[test]
    fn test_delegate() {
        let mut state = StakingState::new();

        state
            .register_validator(
                test_address(1),
                test_address(1),
                1_000_000_000,
                1000,
                test_address(2),
            )
            .unwrap();

        let result = state.delegate(
            test_address(3),
            test_address(3),
            test_address(1),
            500_000_000,
        );

        assert!(result.is_ok());
        assert_eq!(state.delegations.len(), 1);
        assert_eq!(state.total_staked, 1_500_000_000);
    }

    #[test]
    fn test_unbond() {
        let mut state = StakingState::new();

        state
            .register_validator(
                test_address(1),
                test_address(1),
                1_000_000_000,
                1000,
                test_address(2),
            )
            .unwrap();

        state
            .delegate(
                test_address(3),
                test_address(3),
                test_address(1),
                500_000_000,
            )
            .unwrap();

        let result = state.unbond(
            test_address(3),
            test_address(3),
            test_address(1),
            200_000_000,
            1000,
        );

        assert!(result.is_ok());
        assert_eq!(state.unbonding.len(), 1);
        assert_eq!(state.total_staked, 1_300_000_000);
    }

    #[test]
    fn test_slash() {
        let mut state = StakingState::new();

        state
            .register_validator(
                test_address(1),
                test_address(1),
                1_000_000_000,
                1000,
                test_address(2),
            )
            .unwrap();

        // Slash 5%
        let slashed = state.slash(test_address(1), 500).unwrap();

        assert_eq!(slashed, 50_000_000);
        assert_eq!(
            state.get_validator(&test_address(1)).unwrap().staked_amount,
            950_000_000
        );
    }

    #[test]
    fn test_slash_updates_delegation_balances() {
        let mut state = StakingState::new();

        state
            .register_validator(
                test_address(1),
                test_address(1),
                1_000_000_000,
                1000,
                test_address(2),
            )
            .unwrap();
        state
            .delegate(
                test_address(3),
                test_address(3),
                test_address(1),
                500_000_000,
            )
            .unwrap();

        let slashed = state.slash(test_address(1), 5000).unwrap();
        assert_eq!(slashed, 750_000_000);
        assert_eq!(
            state
                .get_validator(&test_address(1))
                .unwrap()
                .delegated_amount,
            250_000_000
        );
        assert_eq!(state.delegations[0].amount, 250_000_000);

        let err = state
            .unbond(
                test_address(3),
                test_address(3),
                test_address(1),
                500_000_000,
                1000,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            StakingError::InsufficientDelegation {
                have: 250_000_000,
                requested: 500_000_000
            }
        ));
    }

    #[test]
    fn test_complete_unbonding() {
        let mut state = StakingState::new();

        state.unbonding.push(Unbonding {
            address: test_address(1),
            amount: 100,
            complete_slot: 1000,
        });

        // Before completion
        let completed = state.complete_unbonding(999);
        assert_eq!(completed.len(), 0);

        // After completion
        let completed = state.complete_unbonding(1000);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].1, 100);
    }

    // ── Adversarial tests ────────────────────────────────────

    #[test]
    fn test_slash_with_high_rate() {
        let mut state = StakingState::new();

        state
            .register_validator(
                test_address(1),
                test_address(1),
                1_000_000_000,
                1000,
                test_address(2),
            )
            .unwrap();

        // Slash at 100% (rate = 10000 bps)
        let slashed = state.slash(test_address(1), 10000).unwrap();
        assert_eq!(slashed, 1_000_000_000);
        assert_eq!(
            state.get_validator(&test_address(1)).unwrap().staked_amount,
            0,
            "staked_amount should be 0 after 100% slash"
        );

        // Slash again at 5% — should not underflow since staked_amount is already 0
        let slashed_again = state.slash(test_address(1), 500).unwrap();
        assert_eq!(slashed_again, 0, "slashing 0 stake should yield 0");
        assert_eq!(
            state.get_validator(&test_address(1)).unwrap().staked_amount,
            0,
            "staked_amount should remain 0 after second slash"
        );
    }

    #[test]
    fn test_slash_rejects_rate_above_100_percent() {
        let mut state = StakingState::new();

        state
            .register_validator(
                test_address(1),
                test_address(1),
                1_000_000_000,
                1000,
                test_address(2),
            )
            .unwrap();

        let result = state.slash(test_address(1), 10_001);
        assert!(matches!(
            result,
            Err(StakingError::InvalidSlashRate(10_001))
        ));
        assert_eq!(
            state.get_validator(&test_address(1)).unwrap().staked_amount,
            1_000_000_000,
            "invalid slash rates must leave stake untouched"
        );
    }
}
