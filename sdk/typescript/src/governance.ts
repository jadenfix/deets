/**
 * Governance helpers for Aether
 * 
 * Simplifies proposal creation, voting, and execution.
 */

import { AetherClient } from './client';
import { AetherKeypair } from './keypair';
import { TransactionBuilder } from './transaction';
import { Address, SignedTransaction, Proposal, Vote } from './types';

// Governance contract address (from genesis)
const GOVERNANCE_CONTRACT = '0x1000000000000000000000000000000000000002';

export class GovernanceHelper {
  constructor(
    private client: AetherClient,
    private keypair?: AetherKeypair
  ) {}

  /**
   * Get proposal by ID
   */
  async getProposal(proposalId: number): Promise<Proposal | null> {
    try {
      return await this.client['call']<Proposal>('governance_getProposal', [proposalId]);
    } catch {
      return null;
    }
  }

  /**
   * Get all active proposals
   */
  async getActiveProposals(): Promise<Proposal[]> {
    return this.client['call']<Proposal[]>('governance_getActiveProposals', []);
  }

  /**
   * Get all proposals (including inactive)
   */
  async getAllProposals(): Promise<Proposal[]> {
    return this.client['call']<Proposal[]>('governance_getAllProposals', []);
  }

  /**
   * Get vote for a proposal
   */
  async getVote(proposalId: number, voter: Address): Promise<Vote | null> {
    try {
      return await this.client['call']<Vote>('governance_getVote', [proposalId, voter]);
    } catch {
      return null;
    }
  }

  /**
   * Create a new proposal
   * 
   * @param title - Proposal title
   * @param description - Proposal description
   * @param duration - Voting duration in slots (default: 100,800 = ~7 days)
   */
  async createProposal(
    title: string,
    description: string,
    duration: number = 100800
  ): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    if (title.length === 0 || title.length > 256) {
      throw new Error('Title must be between 1 and 256 characters');
    }

    if (description.length === 0 || description.length > 10000) {
      throw new Error('Description must be between 1 and 10000 characters');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: createProposal(string title, string description, uint256 duration)
    const data = this.encodeCall('createProposal', [title, description, duration]);
    
    return TransactionBuilder
      .call(this.keypair.address, GOVERNANCE_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Vote on a proposal
   * 
   * @param proposalId - Proposal ID
   * @param support - true for yes, false for no
   */
  async vote(proposalId: number, support: boolean): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: vote(uint256 proposalId, bool support)
    const data = this.encodeCall('vote', [proposalId, support]);
    
    return TransactionBuilder
      .call(this.keypair.address, GOVERNANCE_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Execute a passed proposal
   * 
   * @param proposalId - Proposal ID
   */
  async executeProposal(proposalId: number): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const proposal = await this.getProposal(proposalId);
    if (!proposal) {
      throw new Error('Proposal not found');
    }

    if (proposal.status !== 'passed') {
      throw new Error('Proposal must be in passed state');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: executeProposal(uint256 proposalId)
    const data = this.encodeCall('executeProposal', [proposalId]);
    
    return TransactionBuilder
      .call(this.keypair.address, GOVERNANCE_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Get voting power for an address
   */
  async getVotingPower(address: Address): Promise<bigint> {
    return this.client['call']<bigint>('governance_getVotingPower', [address]);
  }

  /**
   * Get quorum threshold
   */
  async getQuorum(): Promise<bigint> {
    return this.client['call']<bigint>('governance_getQuorum', []);
  }

  /**
   * Check if a proposal has reached quorum
   */
  async hasQuorum(proposalId: number): Promise<boolean> {
    const proposal = await this.getProposal(proposalId);
    if (!proposal) return false;
    
    const quorum = await this.getQuorum();
    const totalVotes = proposal.votesFor + proposal.votesAgainst;
    
    return totalVotes >= quorum;
  }

  /**
   * Get proposal status with context
   */
  async getProposalStatus(proposalId: number): Promise<{
    proposal: Proposal;
    hasQuorum: boolean;
    timeRemaining: number; // slots
    canExecute: boolean;
  } | null> {
    const proposal = await this.getProposal(proposalId);
    if (!proposal) return null;
    
    const currentSlot = await this.client.getSlot();
    const hasQuorum = await this.hasQuorum(proposalId);
    const timeRemaining = Math.max(0, proposal.endSlot - currentSlot);
    const canExecute = proposal.status === 'passed';
    
    return {
      proposal,
      hasQuorum,
      timeRemaining,
      canExecute,
    };
  }

  /**
   * Simple function selector encoding (first 4 bytes of SHA256)
   */
  private encodeCall(method: string, params: any[]): Uint8Array {
    const encoder = new TextEncoder();
    const signature = encoder.encode(method);
    
    // This is a simplified encoding - production would use proper ABI encoding
    const selector = new Uint8Array(4);
    const hash = encoder.encode(method + JSON.stringify(params));
    selector.set(hash.slice(0, 4));
    
    return selector;
  }
}

