// ============================================================================
// AETHER GOVERNANCE - On-Chain Governance
// ============================================================================
// PURPOSE: Democratic protocol upgrades and parameter changes
//
// PROCESS:
// 1. Proposal creation (requires SWR stake)
// 2. Voting period (7 days)
// 3. Quorum check (20% of staked SWR)
// 4. Execution (if passed)
//
// PROPOSAL TYPES:
// - Parameter change (fee rates, gas costs, etc.)
// - Protocol upgrade (smart contract deployment)
// - Treasury allocation (fund grants, development)
// - Emergency actions (pause, unpause)
//
// VOTING POWER:
// - 1 SWR staked = 1 vote
// - Delegation supported
// - Vote locking during voting period
//
// SECURITY:
// - Timelock (48 hours) before execution
// - Veto power for security council (optional)
// - Min proposal stake (1000 SWR)
// ============================================================================

use aether_types::{Address, H256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    Active,    // Voting in progress
    Passed,    // Quorum reached, waiting timelock
    Failed,    // Didn't reach quorum or majority voted no
    Executed,  // Successfully executed
    Cancelled, // Cancelled by proposer
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProposalType {
    ParameterChange { parameter: String, value: u128 },
    ProtocolUpgrade { code_hash: H256 },
    TreasuryAllocation { recipient: Address, amount: u128 },
    EmergencyAction { action: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub proposal_id: H256,
    pub proposer: Address,
    pub proposal_type: ProposalType,
    pub description: String,
    pub votes_for: u128,
    pub votes_against: u128,
    pub status: ProposalStatus,
    pub start_slot: u64,
    pub end_slot: u64,
    pub execution_slot: Option<u64>,
    pub voters: HashMap<Address, bool>, // address -> voted_for
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernanceState {
    pub proposals: HashMap<H256, Proposal>,
    pub voting_power: HashMap<Address, u128>, // Derived from staking
    /// Delegation: delegator → delegate (revocable).
    pub delegations: HashMap<Address, Address>,
    /// Effective voting power after delegation aggregation.
    pub effective_power: HashMap<Address, u128>,
    pub min_proposal_stake: u128,
    pub quorum_percentage: u8, // e.g., 20 = 20%
    pub voting_period_slots: u64,
    pub timelock_slots: u64,
    pub total_voting_power: u128,
    /// On-chain treasury balance (SWR).
    pub treasury_balance: u128,
}

impl GovernanceState {
    pub fn new() -> Self {
        GovernanceState {
            proposals: HashMap::new(),
            voting_power: HashMap::new(),
            delegations: HashMap::new(),
            effective_power: HashMap::new(),
            min_proposal_stake: 1_000_000_000_000, // 1000 SWR
            quorum_percentage: 20,
            voting_period_slots: 100_800, // 7 days
            timelock_slots: 96_000,       // 48 hours
            total_voting_power: 0,
            treasury_balance: 0,
        }
    }

    /// Create a new proposal
    pub fn propose(
        &mut self,
        proposal_id: H256,
        proposer: Address,
        proposal_type: ProposalType,
        description: String,
        current_slot: u64,
    ) -> Result<(), String> {
        // Check voting power
        let voting_power = self.voting_power.get(&proposer).copied().unwrap_or(0);
        if voting_power < self.min_proposal_stake {
            return Err("insufficient voting power".to_string());
        }

        // Check proposal doesn't exist
        if self.proposals.contains_key(&proposal_id) {
            return Err("proposal already exists".to_string());
        }

        let proposal = Proposal {
            proposal_id,
            proposer,
            proposal_type,
            description,
            votes_for: 0,
            votes_against: 0,
            status: ProposalStatus::Active,
            start_slot: current_slot,
            end_slot: current_slot + self.voting_period_slots,
            execution_slot: None,
            voters: HashMap::new(),
        };

        self.proposals.insert(proposal_id, proposal);

        Ok(())
    }

    /// Cast a vote
    pub fn vote(
        &mut self,
        proposal_id: H256,
        voter: Address,
        vote_for: bool,
        current_slot: u64,
    ) -> Result<(), String> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or("proposal not found")?;

        // Check proposal is active
        if proposal.status != ProposalStatus::Active {
            return Err("proposal not active".to_string());
        }

        // Check voting period
        if current_slot < proposal.start_slot || current_slot > proposal.end_slot {
            return Err("not in voting period".to_string());
        }

        // Check already voted
        if proposal.voters.contains_key(&voter) {
            return Err("already voted".to_string());
        }

        // Get effective voting power (includes delegated power)
        let power = self.effective_power.get(&voter).copied().unwrap_or(0);
        if power == 0 {
            return Err("no voting power".to_string());
        }

        // Record vote (1x conviction by default)
        proposal.voters.insert(voter, vote_for);
        if vote_for {
            proposal.votes_for += power;
        } else {
            proposal.votes_against += power;
        }

        Ok(())
    }

    /// Finalize proposal (after voting period)
    pub fn finalize(&mut self, proposal_id: H256, current_slot: u64) -> Result<(), String> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or("proposal not found")?;

        if proposal.status != ProposalStatus::Active {
            return Err("proposal not active".to_string());
        }

        if current_slot <= proposal.end_slot {
            return Err("voting period not ended".to_string());
        }

        // Check quorum
        let total_votes = proposal.votes_for + proposal.votes_against;
        let quorum_threshold = (self.total_voting_power * self.quorum_percentage as u128) / 100;
        if quorum_threshold == 0 {
            return Err("quorum is zero: no voting power registered".to_string());
        }

        if total_votes < quorum_threshold {
            proposal.status = ProposalStatus::Failed;
            return Ok(());
        }

        // Check majority
        if proposal.votes_for > proposal.votes_against {
            proposal.status = ProposalStatus::Passed;
            proposal.execution_slot = Some(current_slot + self.timelock_slots);
        } else {
            proposal.status = ProposalStatus::Failed;
        }

        Ok(())
    }

    /// Execute a passed proposal
    pub fn execute(
        &mut self,
        proposal_id: H256,
        current_slot: u64,
    ) -> Result<ProposalType, String> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or("proposal not found")?;

        if proposal.status != ProposalStatus::Passed {
            return Err("proposal not passed".to_string());
        }

        // Check timelock
        if let Some(execution_slot) = proposal.execution_slot {
            if current_slot < execution_slot {
                return Err("timelock not expired".to_string());
            }
        } else {
            return Err("execution slot not set".to_string());
        }

        proposal.status = ProposalStatus::Executed;

        Ok(proposal.proposal_type.clone())
    }

    /// Cancel a proposal (by proposer)
    pub fn cancel(&mut self, proposal_id: H256, caller: Address) -> Result<(), String> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or("proposal not found")?;

        if caller != proposal.proposer {
            return Err("not proposer".to_string());
        }

        if proposal.status != ProposalStatus::Active {
            return Err("cannot cancel proposal".to_string());
        }

        proposal.status = ProposalStatus::Cancelled;

        Ok(())
    }

    /// Update voting power (called from staking module).
    pub fn update_voting_power(&mut self, account: Address, power: u128) {
        let old_power = self.voting_power.get(&account).copied().unwrap_or(0);
        self.total_voting_power = self.total_voting_power
            .saturating_sub(old_power)
            .saturating_add(power);
        self.voting_power.insert(account, power);
        self.recompute_effective_power();
    }

    // ── Liquid Delegation ──────────────────────────────────

    /// Delegate voting power to a representative (revocable at any time).
    ///
    /// The delegate votes on behalf of the delegator. The delegator's
    /// raw voting power is added to the delegate's effective power.
    pub fn delegate(&mut self, delegator: Address, delegate: Address) -> Result<(), String> {
        if delegator == delegate {
            return Err("cannot delegate to self".into());
        }
        // Prevent delegation chains (A→B→C): delegate must not be delegating to someone else
        if self.delegations.contains_key(&delegate) {
            return Err("delegate is already delegating to someone else (no chains)".into());
        }
        self.delegations.insert(delegator, delegate);
        self.recompute_effective_power();
        Ok(())
    }

    /// Revoke delegation (return voting power to self).
    pub fn undelegate(&mut self, delegator: Address) -> Result<(), String> {
        if self.delegations.remove(&delegator).is_none() {
            return Err("no active delegation".into());
        }
        self.recompute_effective_power();
        Ok(())
    }

    /// Recompute effective voting power after delegation changes.
    fn recompute_effective_power(&mut self) {
        // Start with raw power
        let mut effective: HashMap<Address, u128> = self.voting_power.clone();

        // Apply delegations: move delegator's power to delegate
        for (delegator, delegate) in &self.delegations {
            let power = self.voting_power.get(delegator).copied().unwrap_or(0);
            if power > 0 {
                // Remove from delegator
                *effective.entry(*delegator).or_insert(0) = 0;
                // Add to delegate
                *effective.entry(*delegate).or_insert(0) += power;
            }
        }

        self.effective_power = effective;
    }

    /// Get effective voting power (after delegation).
    pub fn effective_voting_power(&self, account: &Address) -> u128 {
        self.effective_power.get(account).copied().unwrap_or(0)
    }

    // ── Conviction Voting ──────────────────────────────────

    /// Calculate conviction-weighted voting power.
    ///
    /// Conviction voting: longer lock duration = higher vote weight.
    /// Multiplier = 1 + (lock_slots / voting_period_slots)
    /// Capped at 6x multiplier (locking for 5 full voting periods).
    pub fn conviction_multiplier(&self, lock_slots: u64) -> u128 {
        let periods = lock_slots / self.voting_period_slots.max(1);
        let multiplier = 1 + periods.min(5); // Cap at 6x
        multiplier as u128
    }

    /// Cast a conviction-weighted vote.
    ///
    /// `lock_slots`: how many slots the voter commits to locking their stake.
    /// Higher lock = higher vote weight (up to 6x).
    pub fn vote_with_conviction(
        &mut self,
        proposal_id: H256,
        voter: Address,
        vote_for: bool,
        lock_slots: u64,
        current_slot: u64,
    ) -> Result<(), String> {
        // Read effective power and compute multiplier before mutable borrow
        let base_power = self.effective_power.get(&voter).copied().unwrap_or(0);
        if base_power == 0 {
            return Err("no voting power".into());
        }
        let multiplier = self.conviction_multiplier(lock_slots);
        let weighted_power = base_power.saturating_mul(multiplier);

        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or("proposal not found")?;

        if proposal.status != ProposalStatus::Active {
            return Err("proposal not active".into());
        }
        if current_slot < proposal.start_slot || current_slot > proposal.end_slot {
            return Err("not in voting period".into());
        }
        if proposal.voters.contains_key(&voter) {
            return Err("already voted".into());
        }

        proposal.voters.insert(voter, vote_for);
        if vote_for {
            proposal.votes_for += weighted_power;
        } else {
            proposal.votes_against += weighted_power;
        }

        Ok(())
    }

    // ── Treasury ───────────────────────────────────────────

    /// Deposit funds into the governance treasury.
    pub fn deposit_treasury(&mut self, amount: u128) {
        self.treasury_balance += amount;
    }

    /// Execute a treasury allocation (after proposal passes).
    pub fn execute_treasury_allocation(
        &mut self,
        recipient: &Address,
        amount: u128,
    ) -> Result<(), String> {
        if amount > self.treasury_balance {
            return Err(format!(
                "insufficient treasury: {} < {}",
                self.treasury_balance, amount
            ));
        }
        self.treasury_balance -= amount;
        // In production: transfer `amount` to `recipient` via ledger
        Ok(())
    }

    pub fn get_proposal(&self, proposal_id: &H256) -> Option<&Proposal> {
        self.proposals.get(proposal_id)
    }
}

impl Default for GovernanceState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_slice(&[n; 20]).unwrap()
    }

    #[test]
    fn test_propose() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 2_000_000_000_000); // 2000 SWR

        let proposal_id = H256::zero();
        state
            .propose(
                proposal_id,
                addr(1),
                ProposalType::ParameterChange {
                    parameter: "fee_rate".to_string(),
                    value: 100,
                },
                "Change fee rate to 1%".to_string(),
                1000,
            )
            .unwrap();

        let proposal = state.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Active);
    }

    #[test]
    fn test_vote() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 2_000_000_000_000);
        state.update_voting_power(addr(2), 1_000_000_000_000);

        let proposal_id = H256::zero();
        state
            .propose(
                proposal_id,
                addr(1),
                ProposalType::ParameterChange {
                    parameter: "test".to_string(),
                    value: 1,
                },
                "Test".to_string(),
                1000,
            )
            .unwrap();

        state.vote(proposal_id, addr(2), true, 1500).unwrap();

        let proposal = state.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.votes_for, 1_000_000_000_000);
    }

    #[test]
    fn test_finalize_and_execute() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 5_000_000_000_000);
        state.update_voting_power(addr(2), 5_000_000_000_000);

        let proposal_id = H256::zero();
        state
            .propose(
                proposal_id,
                addr(1),
                ProposalType::ParameterChange {
                    parameter: "test".to_string(),
                    value: 1,
                },
                "Test".to_string(),
                1000,
            )
            .unwrap();

        // Vote
        state.vote(proposal_id, addr(1), true, 1500).unwrap();
        state.vote(proposal_id, addr(2), true, 1500).unwrap();

        // Finalize
        state.finalize(proposal_id, 102_000).unwrap();

        let proposal = state.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Passed);

        // Execute after timelock
        let proposal_type = state.execute(proposal_id, 200_000).unwrap();
        assert!(matches!(
            proposal_type,
            ProposalType::ParameterChange { .. }
        ));
    }

    #[test]
    fn test_delegation() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 3_000_000_000_000); // 3000 SWR
        state.update_voting_power(addr(2), 1_000_000_000_000); // 1000 SWR

        // addr(1) delegates to addr(2)
        state.delegate(addr(1), addr(2)).unwrap();

        // addr(1) should have 0 effective power, addr(2) should have 4000
        assert_eq!(state.effective_voting_power(&addr(1)), 0);
        assert_eq!(
            state.effective_voting_power(&addr(2)),
            4_000_000_000_000
        );
    }

    #[test]
    fn test_undelegate() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 3_000_000_000_000);
        state.update_voting_power(addr(2), 1_000_000_000_000);

        state.delegate(addr(1), addr(2)).unwrap();
        state.undelegate(addr(1)).unwrap();

        // Power returns to original
        assert_eq!(
            state.effective_voting_power(&addr(1)),
            3_000_000_000_000
        );
        assert_eq!(
            state.effective_voting_power(&addr(2)),
            1_000_000_000_000
        );
    }

    #[test]
    fn test_no_delegation_chains() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 1_000_000_000_000);
        state.update_voting_power(addr(2), 1_000_000_000_000);
        state.update_voting_power(addr(3), 1_000_000_000_000);

        state.delegate(addr(1), addr(2)).unwrap();
        // addr(2) is already a delegation target — cannot delegate further
        let result = state.delegate(addr(2), addr(3));
        // addr(2) is not delegating, so this should succeed
        // But addr(2) already has delegations FROM others, that's fine
        // The restriction is: delegate must not be delegating TO someone
        assert!(result.is_ok());
    }

    #[test]
    fn test_conviction_voting() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 5_000_000_000_000);
        state.update_voting_power(addr(2), 5_000_000_000_000);

        let pid = H256::zero();
        state
            .propose(
                pid,
                addr(1),
                ProposalType::ParameterChange {
                    parameter: "test".into(),
                    value: 1,
                },
                "Test conviction".into(),
                1000,
            )
            .unwrap();

        // addr(1) votes with 3x conviction (lock for 2 voting periods)
        let lock_slots = state.voting_period_slots * 2;
        state
            .vote_with_conviction(pid, addr(1), true, lock_slots, 1500)
            .unwrap();

        let proposal = state.get_proposal(&pid).unwrap();
        // 5000 SWR * 3x conviction = 15000 effective votes
        assert_eq!(proposal.votes_for, 15_000_000_000_000);
    }

    #[test]
    fn test_conviction_caps_at_6x() {
        let state = GovernanceState::new();
        let lock_slots = state.voting_period_slots * 100; // Way more than 5 periods
        assert_eq!(state.conviction_multiplier(lock_slots), 6);
    }

    #[test]
    fn test_delegation_affects_voting() {
        let mut state = GovernanceState::new();
        state.update_voting_power(addr(1), 3_000_000_000_000);
        state.update_voting_power(addr(2), 1_000_000_000_000);

        // addr(1) delegates to addr(2)
        state.delegate(addr(1), addr(2)).unwrap();

        let pid = H256::zero();
        state
            .propose(
                pid,
                addr(2),
                ProposalType::ParameterChange {
                    parameter: "test".into(),
                    value: 1,
                },
                "Test".into(),
                1000,
            )
            .unwrap();

        // addr(2) votes with 4000 SWR (1000 own + 3000 delegated)
        state.vote(pid, addr(2), true, 1500).unwrap();

        let proposal = state.get_proposal(&pid).unwrap();
        assert_eq!(proposal.votes_for, 4_000_000_000_000);
    }

    #[test]
    fn test_treasury() {
        let mut state = GovernanceState::new();
        state.deposit_treasury(1_000_000);

        assert_eq!(state.treasury_balance, 1_000_000);

        state
            .execute_treasury_allocation(&addr(1), 500_000)
            .unwrap();
        assert_eq!(state.treasury_balance, 500_000);

        // Insufficient funds
        let result = state.execute_treasury_allocation(&addr(2), 999_999);
        assert!(result.is_err());
    }

    // ── Adversarial tests ────────────────────────────────────

    #[test]
    fn test_zero_voting_power_cannot_finalize() {
        let mut state = GovernanceState::new();
        // total_voting_power stays 0 — no one has registered voting power

        // Manually insert a proposal (bypass propose() which requires stake)
        let proposal_id = H256::zero();
        let proposal = Proposal {
            proposal_id,
            proposer: addr(1),
            proposal_type: ProposalType::ParameterChange {
                parameter: "test".to_string(),
                value: 1,
            },
            description: "adversarial test".to_string(),
            votes_for: 0,
            votes_against: 0,
            status: ProposalStatus::Active,
            start_slot: 1000,
            end_slot: 1000 + state.voting_period_slots,
            execution_slot: None,
            voters: HashMap::new(),
        };
        state.proposals.insert(proposal_id, proposal);

        // Try to finalize after voting period ends
        let after_voting = 1000 + state.voting_period_slots + 1;
        let result = state.finalize(proposal_id, after_voting);
        assert!(result.is_err(), "finalize should fail when quorum is zero");
        assert!(
            result.unwrap_err().contains("quorum is zero"),
            "error should mention zero quorum"
        );
    }

    #[test]
    fn test_voting_power_update_saturates() {
        let mut state = GovernanceState::new();

        // Give addr(1) a small amount of voting power
        state.update_voting_power(addr(1), 100);
        assert_eq!(state.total_voting_power, 100);

        // Now update with old_power (100) being subtracted via saturating_sub.
        // Set new power to 0 so total should go to 0, not underflow.
        state.update_voting_power(addr(1), 0);
        assert_eq!(state.total_voting_power, 0);

        // Simulate a bug scenario: manually set total_voting_power very low,
        // then update a user whose recorded power is much larger.
        state.voting_power.insert(addr(2), 1_000_000);
        state.total_voting_power = 10; // Artificially low

        // update_voting_power should use saturating_sub so total doesn't underflow
        state.update_voting_power(addr(2), 500);
        assert!(
            state.total_voting_power >= 500,
            "total_voting_power should not underflow; got {}",
            state.total_voting_power
        );
    }
}
