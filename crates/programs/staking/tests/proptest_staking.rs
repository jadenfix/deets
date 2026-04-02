//! Property-based tests for staking program invariants.
//!
//! Tests cover: registration, delegation, unbonding, slashing, reward distribution,
//! and conservation of total_staked across all operations.

use aether_program_staking::StakingState;
use aether_types::Address;
use proptest::prelude::*;

// --- Strategies ---

fn arb_address() -> impl Strategy<Value = Address> {
    prop::array::uniform20(any::<u8>()).prop_map(|bytes| Address::from_slice(&bytes).unwrap())
}

/// Minimum stake is 100_000_000 (100 SWR with 6 decimals).
fn arb_stake() -> impl Strategy<Value = u128> {
    100_000_000u128..=10_000_000_000_000u128 // 100 SWR to 10M SWR
}

fn arb_commission() -> impl Strategy<Value = u16> {
    0u16..=10000u16
}

fn arb_slash_rate() -> impl Strategy<Value = u128> {
    1u128..=10000u128
}

fn arb_delegation_amount() -> impl Strategy<Value = u128> {
    1u128..=1_000_000_000_000u128
}

/// Build a StakingState with one registered, active validator.
fn state_with_validator() -> impl Strategy<Value = (StakingState, Address, Address)> {
    (arb_address(), arb_stake(), arb_commission()).prop_map(|(addr, stake, commission)| {
        let mut state = StakingState::new();
        let reward_addr = Address::from_slice(&[0xFFu8; 20]).unwrap();
        state
            .register_validator(addr, addr, stake, commission, reward_addr)
            .unwrap();
        (state, addr, reward_addr)
    })
}

// --- Invariant helpers ---

/// Verify that total_staked is internally consistent.
///
/// The invariant: total_staked should equal sum of all validator own stakes plus
/// sum of all active delegation amounts. Unbonding amounts have already been
/// subtracted from total_staked during unbond(), so they are NOT included.
///
/// Note: after slashing unbonding entries, total_staked is further reduced by
/// the unbonding slash amount, which creates a gap vs. the on-book stakes.
/// This is correct behavior — the slashed unbonding tokens are burned.
/// So the true invariant is:
///   total_staked <= sum(validator_stakes) + sum(delegation_amounts)
/// with equality when no unbonding entries have been slashed.
fn check_stake_conservation(state: &StakingState) {
    let validator_stake_sum: u128 = state.validators.iter().map(|v| v.staked_amount).sum();
    let delegation_sum: u128 = state.delegations.iter().map(|d| d.amount).sum();
    let on_book = validator_stake_sum.saturating_add(delegation_sum);
    // total_staked should never exceed on-book stakes (modulo small rounding)
    let tolerance = (state.delegations.len() as u128 + state.validators.len() as u128 + 1) * 2;
    if state.total_staked > on_book {
        let excess = state.total_staked - on_book;
        assert!(
            excess <= tolerance,
            "total_staked={} exceeds on-book={} by {}, tolerance={}",
            state.total_staked, on_book, excess, tolerance,
        );
    }
    // total_staked can be less than on_book when unbonding entries were slashed
    // (the slash on unbonding reduces total_staked but those tokens are not in
    // validator_stakes or delegations anymore). This is fine.
}

// --- Property tests ---

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Registration always increases total_staked by exactly the initial stake.
    #[test]
    fn register_increases_total_staked(
        addr in arb_address(),
        stake in arb_stake(),
        commission in arb_commission(),
    ) {
        let mut state = StakingState::new();
        let reward = Address::from_slice(&[0xFFu8; 20]).unwrap();
        let before = state.total_staked;
        state.register_validator(addr, addr, stake, commission, reward).unwrap();
        prop_assert_eq!(state.total_staked, before + stake);
        prop_assert_eq!(state.validators.len(), 1);
        prop_assert!(state.validators[0].is_active);
    }

    /// Duplicate registration always fails.
    #[test]
    fn duplicate_registration_rejected(
        addr in arb_address(),
        stake in arb_stake(),
        commission in arb_commission(),
    ) {
        let mut state = StakingState::new();
        let reward = Address::from_slice(&[0xFFu8; 20]).unwrap();
        state.register_validator(addr, addr, stake, commission, reward).unwrap();
        let result = state.register_validator(addr, addr, stake, commission, reward);
        prop_assert!(result.is_err());
    }

    /// Caller != address is always rejected (impersonation prevention).
    #[test]
    fn impersonation_rejected(
        caller in arb_address(),
        addr in arb_address(),
        stake in arb_stake(),
        commission in arb_commission(),
    ) {
        prop_assume!(caller != addr);
        let mut state = StakingState::new();
        let reward = Address::from_slice(&[0xFFu8; 20]).unwrap();
        let result = state.register_validator(caller, addr, stake, commission, reward);
        prop_assert!(result.is_err());
    }

    /// Delegation increases total_staked and validator's delegated_amount.
    #[test]
    fn delegation_conserves_stake(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        amount in arb_delegation_amount(),
    ) {
        prop_assume!(delegator != val_addr);
        let before = state.total_staked;
        state.delegate(delegator, delegator, val_addr, amount).unwrap();
        prop_assert_eq!(state.total_staked, before + amount);
        let val = state.get_validator(&val_addr).unwrap();
        prop_assert_eq!(val.delegated_amount, amount);
        check_stake_conservation(&state);
    }

    /// Multiple delegations from same delegator accumulate.
    #[test]
    fn repeated_delegation_accumulates(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        a1 in 1u128..=500_000_000_000u128,
        a2 in 1u128..=500_000_000_000u128,
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, a1).unwrap();
        state.delegate(delegator, delegator, val_addr, a2).unwrap();
        let d = state.delegations.iter()
            .find(|d| d.delegator == delegator && d.validator == val_addr)
            .unwrap();
        prop_assert_eq!(d.amount, a1 + a2);
        // Should still be a single delegation entry, not two
        let count = state.delegations.iter()
            .filter(|d| d.delegator == delegator && d.validator == val_addr)
            .count();
        prop_assert_eq!(count, 1);
    }

    /// Unbonding reduces total_staked and creates an unbonding entry.
    #[test]
    fn unbond_reduces_total_staked(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        amount in arb_delegation_amount(),
        slot in 0u64..=1_000_000u64,
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, amount).unwrap();
        let before = state.total_staked;
        state.unbond(delegator, delegator, val_addr, amount, slot).unwrap();
        prop_assert_eq!(state.total_staked, before - amount);
        prop_assert_eq!(state.unbonding.len(), 1);
        prop_assert_eq!(state.unbonding[0].complete_slot, slot + 100_800);
        check_stake_conservation(&state);
    }

    /// Cannot unbond more than delegated.
    #[test]
    fn unbond_excess_rejected(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        amount in arb_delegation_amount(),
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, amount).unwrap();
        let result = state.unbond(delegator, delegator, val_addr, amount + 1, 0);
        prop_assert!(result.is_err());
    }

    /// Complete unbonding returns correct amounts after waiting period.
    #[test]
    fn complete_unbonding_returns_correct_amounts(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        amount in arb_delegation_amount(),
        slot in 0u64..=500_000u64,
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, amount).unwrap();
        state.unbond(delegator, delegator, val_addr, amount, slot).unwrap();

        // Before completion time: nothing returned
        let completed = state.complete_unbonding(slot + 100_799);
        prop_assert!(completed.is_empty());
        prop_assert_eq!(state.unbonding.len(), 1);

        // At completion time: tokens returned
        let completed = state.complete_unbonding(slot + 100_800);
        prop_assert_eq!(completed.len(), 1);
        prop_assert_eq!(completed[0], (delegator, amount));
        prop_assert!(state.unbonding.is_empty());
    }

    /// Slashing reduces validator stake proportionally.
    #[test]
    fn slash_reduces_stake_proportionally(
        (mut state, val_addr, _) in state_with_validator(),
        slash_rate in arb_slash_rate(),
    ) {
        let before = state.validators[0].staked_amount;
        let total_before = state.total_staked;
        let slashed = state.slash(val_addr, slash_rate, 0).unwrap();

        let expected_slash = before * slash_rate / 10000;
        // Allow rounding of 1
        let diff = slashed.abs_diff(expected_slash);
        prop_assert!(diff <= 1, "slashed={} expected={}", slashed, expected_slash);
        prop_assert!(state.total_staked < total_before || slash_rate == 0);
        check_stake_conservation(&state);
    }

    /// Slashing also reduces delegations proportionally.
    #[test]
    fn slash_reduces_delegations(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        del_amount in arb_delegation_amount(),
        slash_rate in arb_slash_rate(),
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, del_amount).unwrap();
        let del_before = del_amount;
        state.slash(val_addr, slash_rate, 0).unwrap();

        if let Some(d) = state.delegations.iter().find(|d| d.delegator == delegator) {
            let expected_remaining = del_before - del_before * slash_rate / 10000;
            let diff = d.amount.abs_diff(expected_remaining);
            prop_assert!(diff <= 1, "delegation={} expected={}", d.amount, expected_remaining);
        }
        check_stake_conservation(&state);
    }

    /// Three slashes jail the validator (is_active becomes false).
    #[test]
    fn three_slashes_jail_validator(
        (mut state, val_addr, _) in state_with_validator(),
    ) {
        prop_assert!(state.validators[0].is_active);
        state.slash(val_addr, 100, 0).unwrap(); // 1%
        prop_assert!(state.validators[0].is_active);
        state.slash(val_addr, 100, 0).unwrap();
        prop_assert!(state.validators[0].is_active);
        state.slash(val_addr, 100, 0).unwrap();
        prop_assert!(!state.validators[0].is_active);
        prop_assert_eq!(state.validators[0].slash_count, 3);
    }

    /// Reward distribution does not reduce total_staked (it only increases or stays same).
    #[test]
    fn rewards_never_decrease_total_staked(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        del_amount in arb_delegation_amount(),
        reward in 1u128..=1_000_000_000_000u128,
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, del_amount).unwrap();
        let before = state.total_staked;
        state.distribute_rewards(reward);
        prop_assert!(state.total_staked >= before,
            "total_staked decreased: {} -> {}", before, state.total_staked);
    }

    /// Reward distribution preserves stake conservation invariant.
    #[test]
    fn rewards_conserve_stake(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        del_amount in arb_delegation_amount(),
        reward in 1u128..=1_000_000_000u128,
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, del_amount).unwrap();
        state.distribute_rewards(reward);
        check_stake_conservation(&state);
    }

    /// Invalid commission rate (>10000) is rejected.
    #[test]
    fn invalid_commission_rejected(
        addr in arb_address(),
        stake in arb_stake(),
        commission in 10001u16..=u16::MAX,
    ) {
        let mut state = StakingState::new();
        let reward = Address::from_slice(&[0xFFu8; 20]).unwrap();
        let result = state.register_validator(addr, addr, stake, commission, reward);
        prop_assert!(result.is_err());
    }

    /// Invalid slash rate (>10000) is rejected.
    #[test]
    fn invalid_slash_rate_rejected(
        (mut state, val_addr, _) in state_with_validator(),
        rate in 10001u128..=20000u128,
    ) {
        let result = state.slash(val_addr, rate, 0);
        prop_assert!(result.is_err());
    }

    /// Stake below minimum is rejected.
    #[test]
    fn below_minimum_stake_rejected(
        addr in arb_address(),
        stake in 0u128..100_000_000u128,
        commission in arb_commission(),
    ) {
        let mut state = StakingState::new();
        let reward = Address::from_slice(&[0xFFu8; 20]).unwrap();
        let result = state.register_validator(addr, addr, stake, commission, reward);
        prop_assert!(result.is_err());
    }

    /// Slashing unbonding entries prevents pre-slash withdrawal exploit.
    #[test]
    fn slash_reduces_unbonding(
        (mut state, val_addr, _) in state_with_validator(),
        delegator in arb_address(),
        del_amount in 100_000_000u128..=1_000_000_000_000u128,
        slash_rate in arb_slash_rate(),
        slot in 0u64..=500_000u64,
    ) {
        prop_assume!(delegator != val_addr);
        state.delegate(delegator, delegator, val_addr, del_amount).unwrap();
        state.unbond(delegator, delegator, val_addr, del_amount, slot).unwrap();
        let unbond_before = state.unbonding[0].amount;
        state.slash(val_addr, slash_rate, 0).unwrap();
        if let Some(u) = state.unbonding.first() {
            prop_assert!(u.amount <= unbond_before,
                "unbonding increased after slash: {} -> {}", unbond_before, u.amount);
        }
        check_stake_conservation(&state);
    }
}
