// ============================================================================
// AETHER GOVERNANCE PROGRAM - On-Chain Parameter Updates & Upgrades
// ============================================================================
// PURPOSE: Decentralized governance for protocol parameters and code upgrades
//
// VOTING POWER: SWR staked tokens
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    GOVERNANCE SYSTEM                              │
// ├──────────────────────────────────────────────────────────────────┤
// │  Proposal Creation  →  Voting Period  →  Quorum Check            │
// │         ↓                                      ↓                  │
// │  Execution Delay  →  Timelock  →  Height-Gated Activation        │
// │         ↓                                      ↓                  │
// │  Parameter Update / Code Upgrade  →  Node Adoption                │
// └──────────────────────────────────────────────────────────────────┘
//
// PROPOSAL TYPES:
// 1. Parameter Update (fees, rent, consensus params)
// 2. Code Upgrade (new runtime version)
// 3. Treasury Spend
// 4. Emergency Action
//
// STATE:
// ```
// struct Proposal:
//     id: u64
//     proposer: Address
//     title: String
//     description: String
//     proposal_type: ProposalType
//     voting_start: u64
//     voting_end: u64
//     execution_delay: u64
//     votes_for: u128
//     votes_against: u128
//     voters: HashMap<Address, Vote>
//     status: ProposalStatus
//     execution_height: Option<u64>
//
// enum ProposalType:
//     ParameterUpdate { key: String, value: Value }
//     CodeUpgrade { wasm_hash: H256, version: u32 }
//     TreasurySpend { recipient: Address, amount: u128 }
//     Emergency { action: EmergencyAction }
//
// enum ProposalStatus:
//     Voting
//     Passed
//     Rejected
//     Executed
//     Cancelled
// ```
//
// WORKFLOW:
// ```
// fn create_proposal(proposal_type, description):
//     // Require minimum stake to propose
//     stake = get_stake(caller)
//     require(stake >= MIN_PROPOSAL_STAKE)
//     
//     proposal = Proposal {
//         id: next_proposal_id(),
//         proposer: caller,
//         description: description,
//         proposal_type: proposal_type,
//         voting_start: current_slot + VOTING_DELAY,
//         voting_end: current_slot + VOTING_DELAY + VOTING_PERIOD,
//         execution_delay: EXECUTION_DELAY,
//         votes_for: 0,
//         votes_against: 0,
//         status: Voting
//     }
//     
//     store_proposal(proposal)
//     return proposal.id
//
// fn vote(proposal_id, vote_choice):
//     proposal = get_proposal(proposal_id)
//     require(proposal.status == Voting)
//     require(current_slot >= proposal.voting_start)
//     require(current_slot < proposal.voting_end)
//     
//     // Voting power = staked SWR
//     voting_power = get_stake(caller)
//     
//     // Record vote
//     proposal.voters[caller] = vote_choice
//     
//     match vote_choice:
//         For:
//             proposal.votes_for += voting_power
//         Against:
//             proposal.votes_against += voting_power
//
// fn finalize_proposal(proposal_id):
//     proposal = get_proposal(proposal_id)
//     require(current_slot >= proposal.voting_end)
//     require(proposal.status == Voting)
//     
//     total_votes = proposal.votes_for + proposal.votes_against
//     
//     // Quorum check (e.g., 40% of total stake)
//     if total_votes < total_staked_swr * QUORUM_THRESHOLD:
//         proposal.status = Rejected
//         return
//     
//     // Approval threshold (e.g., 60% of votes)
//     if proposal.votes_for >= total_votes * APPROVAL_THRESHOLD:
//         proposal.status = Passed
//         proposal.execution_height = current_slot + proposal.execution_delay
//     else:
//         proposal.status = Rejected
//
// fn execute_proposal(proposal_id):
//     proposal = get_proposal(proposal_id)
//     require(proposal.status == Passed)
//     require(current_slot >= proposal.execution_height)
//     
//     match proposal.proposal_type:
//         ParameterUpdate { key, value }:
//             update_parameter(key, value)
//         
//         CodeUpgrade { wasm_hash, version }:
//             schedule_upgrade(wasm_hash, version, proposal.execution_height)
//         
//         TreasurySpend { recipient, amount }:
//             transfer_from_treasury(recipient, amount)
//         
//         Emergency { action }:
//             execute_emergency_action(action)
//     
//     proposal.status = Executed
// ```
//
// PARAMETERS:
// - MIN_PROPOSAL_STAKE: 10,000,000 SWR (1% of supply)
// - VOTING_DELAY: 86400 slots (12 hours discussion)
// - VOTING_PERIOD: 604800 slots (7 days)
// - EXECUTION_DELAY: 172800 slots (24 hours timelock)
// - QUORUM_THRESHOLD: 40% of staked tokens
// - APPROVAL_THRESHOLD: 60% of votes
//
// UPDATABLE PARAMETERS:
// - Fees (a, b, c, d coefficients)
// - Rent (rho, horizon)
// - Consensus (tau, slashing rates)
// - VCR (challenge window, bond minimum)
//
// SECURITY:
// - Timelock prevents instant changes
// - High quorum prevents minority attacks
// - Emergency multisig for critical patches
//
// OUTPUTS:
// - Parameter updates → Node configuration reload
// - Code upgrades → Runtime version activation
// - Treasury spending → Fund development/grants
// ============================================================================

pub mod proposals;
pub mod voting;
pub mod execution;
pub mod treasury;

pub use proposals::Proposal;

