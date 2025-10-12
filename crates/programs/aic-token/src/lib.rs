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

    /// Mint new tokens
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
    pub fn burn(&mut self, from: Address, amount: u128) -> Result<(), String> {
        let balance = self.balances.get_mut(&from).ok_or("insufficient balance")?;

        if *balance < amount {
            return Err("insufficient balance".to_string());
        }

        *balance -= amount;
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

        *from_balance -= amount;

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
            .or_insert_with(HashMap::new)
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

        *allowance -= amount;

        // Transfer
        self.transfer(from, to, amount)?;

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
        state.burn(addr(2), 300).unwrap();

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
}
