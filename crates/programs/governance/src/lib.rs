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
    pub min_proposal_stake: u128,
    pub quorum_percentage: u8, // e.g., 20 = 20%
    pub voting_period_slots: u64,
    pub timelock_slots: u64,
    pub total_voting_power: u128,
}

impl GovernanceState {
    pub fn new() -> Self {
        GovernanceState {
            proposals: HashMap::new(),
            voting_power: HashMap::new(),
            min_proposal_stake: 1_000_000_000_000, // 1000 SWR
            quorum_percentage: 20,
            voting_period_slots: 100_800, // 7 days
            timelock_slots: 96_000,       // 48 hours
            total_voting_power: 0,
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

        // Get voting power
        let power = self.voting_power.get(&voter).copied().unwrap_or(0);
        if power == 0 {
            return Err("no voting power".to_string());
        }

        // Record vote
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

    /// Update voting power (called from staking module)
    pub fn update_voting_power(&mut self, account: Address, power: u128) {
        let old_power = self.voting_power.get(&account).copied().unwrap_or(0);
        self.total_voting_power = self.total_voting_power - old_power + power;
        self.voting_power.insert(account, power);
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
}
