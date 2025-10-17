/**
 * Aether SDK - TypeScript
 * 
 * Official TypeScript SDK for Aether Blockchain
 */

export { AetherClient } from './client';
export { AetherKeypair } from './keypair';
export { TransactionBuilder, TransactionHelper } from './transaction';
export { StakingHelper } from './staking';
export { GovernanceHelper } from './governance';
export { AIJobHelper, ModelHelper } from './ai';

export * from './types';

/**
 * SDK version
 */
export const VERSION = '0.1.0';

/**
 * Convenience exports for common use cases
 */
export const AetherSDK = {
  /**
   * Create a new Aether client
   */
  createClient: (rpcUrl: string) => new (require('./client').AetherClient)({ rpcUrl }),
  
  /**
   * Generate a new keypair
   */
  generateKeypair: () => require('./keypair').AetherKeypair.generate(),
  
  /**
   * Version info
   */
  version: VERSION,
};

