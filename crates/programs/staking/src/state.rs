use aether_types::{Address, H256};
use serde::{Deserialize, Serialize};

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

    /// Register a new validator
    pub fn register_validator(
        &mut self,
        address: Address,
        initial_stake: u128,
        commission_rate: u16,
        reward_address: Address,
    ) -> Result<(), String> {
        // Check minimum stake
        if initial_stake < 100_000_000 {
            // 100 SWR with 6 decimals
            return Err("insufficient stake".to_string());
        }

        // Check validator doesn't exist
        if self.validators.iter().any(|v| v.address == address) {
            return Err("validator already exists".to_string());
        }

        // Check commission rate
        if commission_rate > 10000 {
            return Err("invalid commission rate".to_string());
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
        self.total_staked += initial_stake;

        Ok(())
    }

    /// Delegate to a validator
    pub fn delegate(
        &mut self,
        delegator: Address,
        validator: Address,
        amount: u128,
    ) -> Result<(), String> {
        // Find validator
        let validator_idx = self
            .validators
            .iter()
            .position(|v| v.address == validator)
            .ok_or("validator not found")?;

        // Check validator is active
        if !self.validators[validator_idx].is_active {
            return Err("validator is not active".to_string());
        }

        // Create or update delegation
        if let Some(delegation) = self
            .delegations
            .iter_mut()
            .find(|d| d.delegator == delegator && d.validator == validator)
        {
            delegation.amount += amount;
        } else {
            self.delegations.push(Delegation {
                delegator,
                validator,
                amount,
                reward_debt: 0,
            });
        }

        // Update validator
        self.validators[validator_idx].delegated_amount += amount;
        self.total_staked += amount;

        Ok(())
    }

    /// Unbond stake (start unbonding period)
    pub fn unbond(
        &mut self,
        delegator: Address,
        validator: Address,
        amount: u128,
        current_slot: u64,
    ) -> Result<(), String> {
        // Find delegation
        let delegation = self
            .delegations
            .iter_mut()
            .find(|d| d.delegator == delegator && d.validator == validator)
            .ok_or("delegation not found")?;

        // Check amount
        if amount > delegation.amount {
            return Err("insufficient delegation".to_string());
        }

        // Update delegation
        delegation.amount -= amount;

        // Remove if zero
        if delegation.amount == 0 {
            self.delegations
                .retain(|d| !(d.delegator == delegator && d.validator == validator));
        }

        // Update validator
        if let Some(v) = self.validators.iter_mut().find(|v| v.address == validator) {
            v.delegated_amount -= amount;
        }

        // Add to unbonding queue (7 days = 100,800 slots at 500ms/slot)
        self.unbonding.push(Unbonding {
            address: delegator,
            amount,
            complete_slot: current_slot + 100_800,
        });

        self.total_staked -= amount;

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

    /// Slash a validator for misbehavior
    pub fn slash(
        &mut self,
        validator: Address,
        slash_rate: u128, // Basis points (e.g., 500 = 5%)
    ) -> Result<u128, String> {
        let v = self
            .validators
            .iter_mut()
            .find(|v| v.address == validator)
            .ok_or("validator not found")?;

        // Calculate slash amount
        let slash_amount = v.staked_amount * slash_rate / 10000;

        // Apply slash
        v.staked_amount -= slash_amount;
        v.slash_count += 1;
        self.total_staked -= slash_amount;

        // Jail validator if slashed too many times
        if v.slash_count >= 3 {
            v.is_active = false;
        }

        Ok(slash_amount)
    }

    /// Distribute rewards
    pub fn distribute_rewards(&mut self, epoch_rewards: u128) {
        self.reward_pool += epoch_rewards;

        // Distribute proportionally to validators
        for validator in &mut self.validators {
            if !validator.is_active {
                continue;
            }

            let total_stake = validator.staked_amount + validator.delegated_amount;
            if total_stake == 0 {
                continue;
            }

            let validator_reward = (epoch_rewards * total_stake) / self.total_staked;

            // Commission for validator
            let commission = (validator_reward * validator.commission_rate as u128) / 10000;

            // Rest goes to delegators
            let delegator_reward = validator_reward - commission;

            // In production: distribute to delegators proportionally
            validator.staked_amount += commission;
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
            test_address(1),
            1_000_000_000, // 1000 SWR
            1000,          // 10% commission
            test_address(2),
        );

        assert!(result.is_ok());
        assert_eq!(state.validators.len(), 1);
        assert_eq!(state.total_staked, 1_000_000_000);
    }

    #[test]
    fn test_delegate() {
        let mut state = StakingState::new();

        state
            .register_validator(test_address(1), 1_000_000_000, 1000, test_address(2))
            .unwrap();

        let result = state.delegate(test_address(3), test_address(1), 500_000_000);

        assert!(result.is_ok());
        assert_eq!(state.delegations.len(), 1);
        assert_eq!(state.total_staked, 1_500_000_000);
    }

    #[test]
    fn test_unbond() {
        let mut state = StakingState::new();

        state
            .register_validator(test_address(1), 1_000_000_000, 1000, test_address(2))
            .unwrap();

        state
            .delegate(test_address(3), test_address(1), 500_000_000)
            .unwrap();

        let result = state.unbond(test_address(3), test_address(1), 200_000_000, 1000);

        assert!(result.is_ok());
        assert_eq!(state.unbonding.len(), 1);
        assert_eq!(state.total_staked, 1_300_000_000);
    }

    #[test]
    fn test_slash() {
        let mut state = StakingState::new();

        state
            .register_validator(test_address(1), 1_000_000_000, 1000, test_address(2))
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
}
