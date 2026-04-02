// ============================================================================
// AETHER AIC TOKEN - AI Credits
// ============================================================================
// PURPOSE: Consumable token for AI inference jobs
//
// ECONOMICS:
// - Burned on use (deflationary)
// - Minted through: staking rewards, purchase with SWR
// - Used for: AI inference requests
// - Price discovery: AMM vs SWR
//
// OPERATIONS:
// - mint: Create new AIC (governance controlled)
// - burn: Destroy AIC (automatic on job execution)
// - transfer: Send AIC between accounts
// - allowance: Approve spending (for contracts)
//
// SUPPLY:
// - No hard cap
// - Burn rate adjusts based on network usage
// - Mint rate controlled by governance
//
// INTEGRATION:
// - Job escrow: Burns AIC on completion
// - Staking: Earns AIC rewards
// - AMM: AIC/SWR trading pair
// ============================================================================

use aether_types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AicTokenState {
    /// Total supply
    pub total_supply: u128,

    /// Total burned
    pub total_burned: u128,

    /// Balances
    pub balances: HashMap<Address, u128>,

    /// Allowances (owner -> spender -> amount)
    pub allowances: HashMap<Address, HashMap<Address, u128>>,

    /// Mint authority
    pub mint_authority: Address,
}

impl AicTokenState {
    pub fn new(mint_authority: Address) -> Self {
        AicTokenState {
            total_supply: 0,
            total_burned: 0,
            balances: HashMap::new(),
            allowances: HashMap::new(),
            mint_authority,
        }
    }

    /// Mint new tokens.
    ///
    /// Only the `mint_authority` can mint. There is currently no supply cap
    /// enforced at the program level — governance should impose minting limits
    /// to prevent unchecked inflation.
    pub fn mint(&mut self, caller: Address, to: Address, amount: u128) -> Result<(), String> {
        if caller != self.mint_authority {
            return Err("unauthorized".to_string());
        }

        let balance = self.balances.entry(to).or_insert(0);
        *balance = balance.checked_add(amount).ok_or("overflow")?;

        self.total_supply = self.total_supply.checked_add(amount).ok_or("overflow")?;

        Ok(())
    }

    /// Burn tokens (destroy permanently)
    pub fn burn(&mut self, caller: Address, from: Address, amount: u128) -> Result<(), String> {
        // Only the token owner or an approved spender can burn
        if caller != from {
            // Check allowance
            let allowance = self
                .allowances
                .get_mut(&from)
                .and_then(|m| m.get_mut(&caller))
                .ok_or("unauthorized: caller is not owner and has no allowance")?;
            if *allowance < amount {
                return Err("insufficient allowance for burn".to_string());
            }
            *allowance = allowance.checked_sub(amount).ok_or("allowance underflow")?;
        }

        let balance = self.balances.get_mut(&from).ok_or("insufficient balance")?;

        if *balance < amount {
            return Err("insufficient balance".to_string());
        }

        *balance = balance.checked_sub(amount).ok_or("burn underflow")?;
        self.total_supply = self.total_supply.checked_sub(amount).ok_or("underflow")?;
        self.total_burned = self.total_burned.checked_add(amount).ok_or("overflow")?;

        Ok(())
    }

    /// Transfer tokens
    pub fn transfer(&mut self, from: Address, to: Address, amount: u128) -> Result<(), String> {
        if from == to {
            return Err("cannot transfer to self".to_string());
        }

        let from_balance = self.balances.get_mut(&from).ok_or("insufficient balance")?;

        if *from_balance < amount {
            return Err("insufficient balance".to_string());
        }

        *from_balance = from_balance
            .checked_sub(amount)
            .ok_or("balance underflow")?;

        let to_balance = self.balances.entry(to).or_insert(0);
        *to_balance = to_balance.checked_add(amount).ok_or("overflow")?;

        Ok(())
    }

    /// Approve spending
    pub fn approve(
        &mut self,
        owner: Address,
        spender: Address,
        amount: u128,
    ) -> Result<(), String> {
        self.allowances
            .entry(owner)
            .or_default()
            .insert(spender, amount);

        Ok(())
    }

    /// Transfer from (using allowance)
    pub fn transfer_from(
        &mut self,
        caller: Address,
        from: Address,
        to: Address,
        amount: u128,
    ) -> Result<(), String> {
        // Check allowance
        let allowance = self
            .allowances
            .get_mut(&from)
            .and_then(|m| m.get_mut(&caller))
            .ok_or("insufficient allowance")?;

        if *allowance < amount {
            return Err("insufficient allowance".to_string());
        }

        // Verify allowance without holding the mutable borrow across the transfer call.
        let current = *allowance;
        let new_allowance = current.checked_sub(amount).ok_or("allowance underflow")?;
        // Release the mutable borrow so `self.transfer` can take `&mut self`.

        // Attempt transfer BEFORE committing the allowance deduction so that a
        // failed transfer does not silently consume the caller's allowance.
        self.transfer(from, to, amount)?;

        // Transfer succeeded — now commit the allowance deduction.
        if let Some(entry) = self.allowances.get_mut(&from).and_then(|m| m.get_mut(&caller)) {
            *entry = new_allowance;
        }
        Ok(())
    }

    pub fn balance_of(&self, account: &Address) -> u128 {
        self.balances.get(account).copied().unwrap_or(0)
    }

    pub fn allowance_of(&self, owner: &Address, spender: &Address) -> u128 {
        self.allowances
            .get(owner)
            .and_then(|m| m.get(spender))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_slice(&[n; 20]).unwrap()
    }

    #[test]
    fn test_mint() {
        let mut state = AicTokenState::new(addr(1));

        state.mint(addr(1), addr(2), 1000).unwrap();

        assert_eq!(state.balance_of(&addr(2)), 1000);
        assert_eq!(state.total_supply, 1000);
    }

    #[test]
    fn test_burn() {
        let mut state = AicTokenState::new(addr(1));

        state.mint(addr(1), addr(2), 1000).unwrap();
        state.burn(addr(2), addr(2), 300).unwrap();

        assert_eq!(state.balance_of(&addr(2)), 700);
        assert_eq!(state.total_burned, 300);
        assert_eq!(state.total_supply, 700);
    }

    #[test]
    fn test_transfer() {
        let mut state = AicTokenState::new(addr(1));

        state.mint(addr(1), addr(2), 1000).unwrap();
        state.transfer(addr(2), addr(3), 400).unwrap();

        assert_eq!(state.balance_of(&addr(2)), 600);
        assert_eq!(state.balance_of(&addr(3)), 400);
    }

    #[test]
    fn test_approve_and_transfer_from() {
        let mut state = AicTokenState::new(addr(1));

        state.mint(addr(1), addr(2), 1000).unwrap();
        state.approve(addr(2), addr(3), 500).unwrap();
        state.transfer_from(addr(3), addr(2), addr(4), 300).unwrap();

        assert_eq!(state.balance_of(&addr(2)), 700);
        assert_eq!(state.balance_of(&addr(4)), 300);
        assert_eq!(state.allowance_of(&addr(2), &addr(3)), 200);
    }

    // ── Adversarial tests ────────────────────────────────────

    #[test]
    fn test_burn_more_than_balance_rejected() {
        let mut state = AicTokenState::new(addr(1));

        state.mint(addr(1), addr(2), 100).unwrap();

        let result = state.burn(addr(2), addr(2), 200);
        assert!(result.is_err(), "burning more than balance should fail");

        // Balance and supply unchanged
        assert_eq!(state.balance_of(&addr(2)), 100);
        assert_eq!(state.total_supply, 100);
    }

    #[test]
    fn test_transfer_from_exceeds_allowance_rejected() {
        let mut state = AicTokenState::new(addr(1));

        state.mint(addr(1), addr(2), 1000).unwrap();
        state.approve(addr(2), addr(3), 50).unwrap();

        let result = state.transfer_from(addr(3), addr(2), addr(4), 100);
        assert!(
            result.is_err(),
            "transfer_from exceeding allowance should fail"
        );

        // Nothing changed
        assert_eq!(state.balance_of(&addr(2)), 1000);
        assert_eq!(state.balance_of(&addr(4)), 0);
        assert_eq!(state.allowance_of(&addr(2), &addr(3)), 50);
    }

    /// transfer_from must NOT consume the allowance when the underlying
    /// transfer fails (e.g. sender has insufficient balance).
    #[test]
    fn test_transfer_from_does_not_consume_allowance_on_failed_transfer() {
        let mut state = AicTokenState::new(addr(1));

        // addr(2) has 0 balance but addr(3) has been granted an allowance of 500
        state.approve(addr(2), addr(3), 500).unwrap();

        let result = state.transfer_from(addr(3), addr(2), addr(4), 300);
        assert!(result.is_err(), "transfer should fail: sender has no balance");

        // Allowance must be fully preserved — it was not consumed by the failed transfer
        assert_eq!(
            state.allowance_of(&addr(2), &addr(3)),
            500,
            "allowance must not be consumed when transfer fails"
        );
        assert_eq!(state.balance_of(&addr(4)), 0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_addr() -> impl Strategy<Value = Address> {
        // Use addresses 1..10 to get interesting collisions/distinct actors
        (1u8..10u8).prop_map(|n| Address::from_slice(&[n; 20]).unwrap())
    }

    proptest! {
        /// mint increases balance of recipient and total_supply by exactly `amount`.
        #[test]
        fn mint_increases_balance_and_supply(amount in 0u128..1_000_000u128) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let recipient = Address::from_slice(&[2u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);

            let before_supply = state.total_supply;
            state.mint(authority, recipient, amount).unwrap();

            prop_assert_eq!(state.balance_of(&recipient), amount);
            prop_assert_eq!(state.total_supply, before_supply + amount);
        }

        /// burn decreases balance and total_supply, increases total_burned.
        #[test]
        fn burn_decreases_supply_increases_burned(
            mint_amt in 1u128..1_000_000u128,
            burn_frac in 0.0f64..=1.0f64,
        ) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let holder = Address::from_slice(&[2u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            state.mint(authority, holder, mint_amt).unwrap();

            let burn_amt = (mint_amt as f64 * burn_frac) as u128;
            state.burn(holder, holder, burn_amt).unwrap();

            prop_assert_eq!(state.balance_of(&holder), mint_amt - burn_amt);
            prop_assert_eq!(state.total_supply, mint_amt - burn_amt);
            prop_assert_eq!(state.total_burned, burn_amt);
        }

        /// transfer conserves total supply: sum of balances stays constant.
        #[test]
        fn transfer_conserves_supply(
            mint_amt in 1u128..1_000_000u128,
            transfer_frac in 0.0f64..=1.0f64,
        ) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let sender = Address::from_slice(&[2u8; 20]).unwrap();
            let receiver = Address::from_slice(&[3u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            state.mint(authority, sender, mint_amt).unwrap();

            let transfer_amt = (mint_amt as f64 * transfer_frac) as u128;
            state.transfer(sender, receiver, transfer_amt).unwrap();

            let total = state.balance_of(&sender) + state.balance_of(&receiver);
            prop_assert_eq!(total, mint_amt);
            prop_assert_eq!(state.total_supply, mint_amt);
        }

        /// transfer_from respects allowance and reduces it correctly.
        #[test]
        fn transfer_from_reduces_allowance(
            mint_amt in 100u128..1_000_000u128,
            allowance in 10u128..100u128,
            transfer_amt in 1u128..10u128,
        ) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let owner = Address::from_slice(&[2u8; 20]).unwrap();
            let spender = Address::from_slice(&[3u8; 20]).unwrap();
            let dest = Address::from_slice(&[4u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            state.mint(authority, owner, mint_amt).unwrap();
            state.approve(owner, spender, allowance).unwrap();

            state.transfer_from(spender, owner, dest, transfer_amt).unwrap();

            prop_assert_eq!(state.allowance_of(&owner, &spender), allowance - transfer_amt);
            prop_assert_eq!(state.balance_of(&dest), transfer_amt);
            prop_assert_eq!(state.balance_of(&owner), mint_amt - transfer_amt);
        }

        /// Unauthorized mint is rejected.
        #[test]
        fn unauthorized_mint_rejected(impostor in arb_addr(), amount in 0u128..1_000_000u128) {
            let authority = Address::from_slice(&[99u8; 20]).unwrap();
            prop_assume!(impostor != authority);
            let recipient = Address::from_slice(&[2u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);

            let result = state.mint(impostor, recipient, amount);
            prop_assert!(result.is_err(), "non-authority mint must be rejected");
            prop_assert_eq!(state.total_supply, 0);
        }

        /// Burning more than balance is rejected; state remains unchanged.
        #[test]
        fn burn_more_than_balance_rejected(
            mint_amt in 1u128..1_000_000u128,
            extra in 1u128..100_000u128,
        ) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let holder = Address::from_slice(&[2u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            state.mint(authority, holder, mint_amt).unwrap();

            let result = state.burn(holder, holder, mint_amt + extra);
            prop_assert!(result.is_err());
            prop_assert_eq!(state.balance_of(&holder), mint_amt);
            prop_assert_eq!(state.total_supply, mint_amt);
            prop_assert_eq!(state.total_burned, 0);
        }

        /// Transferring more than balance is rejected; neither balance changes.
        #[test]
        fn transfer_more_than_balance_rejected(
            mint_amt in 1u128..1_000_000u128,
            extra in 1u128..100_000u128,
        ) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let sender = Address::from_slice(&[2u8; 20]).unwrap();
            let receiver = Address::from_slice(&[3u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            state.mint(authority, sender, mint_amt).unwrap();

            let result = state.transfer(sender, receiver, mint_amt + extra);
            prop_assert!(result.is_err());
            prop_assert_eq!(state.balance_of(&sender), mint_amt);
            prop_assert_eq!(state.balance_of(&receiver), 0);
        }

        /// Multiple mints accumulate correctly.
        #[test]
        fn multiple_mints_accumulate(amounts in prop::collection::vec(0u128..100_000u128, 1..10)) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let recipient = Address::from_slice(&[2u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);

            let mut expected: u128 = 0;
            for &amt in &amounts {
                state.mint(authority, recipient, amt).unwrap();
                expected = expected.saturating_add(amt);
            }
            prop_assert_eq!(state.balance_of(&recipient), expected);
            prop_assert_eq!(state.total_supply, expected);
        }

        /// total_supply == sum of all balances at all times.
        #[test]
        fn supply_equals_sum_of_balances(
            mint_amt in 100u128..1_000_000u128,
            transfer_amt in 0u128..100u128,
            burn_amt in 0u128..100u128,
        ) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let a = Address::from_slice(&[2u8; 20]).unwrap();
            let b = Address::from_slice(&[3u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            state.mint(authority, a, mint_amt).unwrap();

            // transfer some (cap to balance)
            let t = transfer_amt.min(mint_amt);
            state.transfer(a, b, t).unwrap();

            // burn some from a (cap to remaining balance)
            let burn = burn_amt.min(mint_amt - t);
            state.burn(a, a, burn).unwrap();

            let sum_balances: u128 = state.balances.values().sum();
            prop_assert_eq!(state.total_supply, sum_balances,
                "total_supply must equal sum of all balances");
        }

        /// transfer_from does NOT consume allowance when the transfer fails.
        #[test]
        fn transfer_from_no_allowance_consumed_on_failure(allowance in 1u128..100_000u128) {
            let authority = Address::from_slice(&[1u8; 20]).unwrap();
            let owner = Address::from_slice(&[2u8; 20]).unwrap();
            let spender = Address::from_slice(&[3u8; 20]).unwrap();
            let dest = Address::from_slice(&[4u8; 20]).unwrap();
            let mut state = AicTokenState::new(authority);
            // owner has 0 balance but has granted allowance
            state.approve(owner, spender, allowance).unwrap();

            let result = state.transfer_from(spender, owner, dest, allowance);
            prop_assert!(result.is_err(), "should fail: owner has no balance");
            prop_assert_eq!(state.allowance_of(&owner, &spender), allowance,
                "allowance must not be consumed on failed transfer");
        }
    }
}
