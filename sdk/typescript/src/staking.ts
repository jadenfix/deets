/**
 * Staking helpers for Aether
 * 
 * Simplifies validator registration, delegation, and reward claiming.
 */

import { AetherClient } from './client';
import { AetherKeypair } from './keypair';
import { TransactionBuilder } from './transaction';
import { Address, SignedTransaction, Validator, Delegation } from './types';

// Staking contract address (from genesis)
const STAKING_CONTRACT = '0x1000000000000000000000000000000000000001';

export class StakingHelper {
  constructor(
    private client: AetherClient,
    private keypair?: AetherKeypair
  ) {}

  /**
   * Get validator information
   */
  async getValidator(address: Address): Promise<Validator | null> {
    try {
      return await this.client['call']<Validator>('staking_getValidator', [address]);
    } catch {
      return null;
    }
  }

  /**
   * Get all active validators
   */
  async getValidators(): Promise<Validator[]> {
    return this.client['call']<Validator[]>('staking_getValidators', []);
  }

  /**
   * Get delegation information
   */
  async getDelegation(delegator: Address, validator: Address): Promise<Delegation | null> {
    try {
      return await this.client['call']<Delegation>('staking_getDelegation', [delegator, validator]);
    } catch {
      return null;
    }
  }

  /**
   * Get all delegations for a delegator
   */
  async getDelegations(delegator: Address): Promise<Delegation[]> {
    return this.client['call']<Delegation[]>('staking_getDelegations', [delegator]);
  }

  /**
   * Register as a validator
   * 
   * @param stake - Initial stake amount (must be >= minimum stake)
   * @param commission - Commission rate in basis points (0-10000)
   */
  async registerValidator(stake: bigint, commission: number): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }
    
    if (commission < 0 || commission > 10000) {
      throw new Error('Commission must be between 0 and 10000 basis points');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: registerValidator(uint256 commission)
    const data = this.encodeCall('registerValidator', [commission]);
    
    return TransactionBuilder
      .call(this.keypair.address, STAKING_CONTRACT, data, nonce, stake)
      .sign(this.keypair);
  }

  /**
   * Delegate stake to a validator
   * 
   * @param validator - Validator address
   * @param amount - Amount to delegate
   */
  async delegate(validator: Address, amount: bigint): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: delegate(address validator)
    const data = this.encodeCall('delegate', [validator]);
    
    return TransactionBuilder
      .call(this.keypair.address, STAKING_CONTRACT, data, nonce, amount)
      .sign(this.keypair);
  }

  /**
   * Undelegate stake from a validator
   * 
   * @param validator - Validator address
   * @param amount - Amount to undelegate
   */
  async undelegate(validator: Address, amount: bigint): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: undelegate(address validator, uint256 amount)
    const data = this.encodeCall('undelegate', [validator, amount]);
    
    return TransactionBuilder
      .call(this.keypair.address, STAKING_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Claim staking rewards
   */
  async claimRewards(): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: claimRewards()
    const data = this.encodeCall('claimRewards', []);
    
    return TransactionBuilder
      .call(this.keypair.address, STAKING_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Get pending rewards
   */
  async getPendingRewards(address: Address): Promise<bigint> {
    return this.client['call']<bigint>('staking_getPendingRewards', [address]);
  }

  /**
   * Get total staked amount in the network
   */
  async getTotalStake(): Promise<bigint> {
    return this.client['call']<bigint>('staking_getTotalStake', []);
  }

  /**
   * Get minimum stake requirement
   */
  async getMinimumStake(): Promise<bigint> {
    return this.client['call']<bigint>('staking_getMinimumStake', []);
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

