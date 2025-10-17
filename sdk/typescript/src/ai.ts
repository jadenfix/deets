/**
 * AI Job submission and Verifiable Compute Receipt tracking
 * 
 * Core functionality for Aether's AI marketplace.
 */

import { AetherClient } from './client';
import { AetherKeypair } from './keypair';
import { TransactionBuilder } from './transaction';
import { Address, SignedTransaction, AIJob, VerifiableComputeReceipt, Hash, Bytes } from './types';

// Job escrow contract address (from genesis)
const JOB_ESCROW_CONTRACT = '0x1000000000000000000000000000000000000003';

export class AIJobHelper {
  constructor(
    private client: AetherClient,
    private keypair?: AetherKeypair
  ) {}

  /**
   * Get job by ID
   */
  async getJob(jobId: Hash): Promise<AIJob | null> {
    try {
      return await this.client['call']<AIJob>('ai_getJob', [jobId]);
    } catch {
      return null;
    }
  }

  /**
   * Get all jobs for a creator
   */
  async getJobsByCreator(creator: Address): Promise<AIJob[]> {
    return this.client['call']<AIJob[]>('ai_getJobsByCreator', [creator]);
  }

  /**
   * Get all pending jobs (available for providers)
   */
  async getPendingJobs(): Promise<AIJob[]> {
    return this.client['call']<AIJob[]>('ai_getPendingJobs', []);
  }

  /**
   * Get jobs assigned to a provider
   */
  async getJobsByProvider(provider: Address): Promise<AIJob[]> {
    return this.client['call']<AIJob[]>('ai_getJobsByProvider', [provider]);
  }

  /**
   * Submit a new AI job
   * 
   * @param modelHash - Hash of the model to execute
   * @param inputData - Input data for the model
   * @param aicAmount - Amount of AIC tokens to lock as payment
   * @returns Signed transaction
   */
  async submitJob(
    modelHash: Hash,
    inputData: Bytes,
    aicAmount: bigint
  ): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    if (aicAmount <= 0n) {
      throw new Error('AIC amount must be positive');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: submitJob(bytes32 modelHash, bytes inputData)
    const data = this.encodeJobSubmission(modelHash, inputData);
    
    return TransactionBuilder
      .call(this.keypair.address, JOB_ESCROW_CONTRACT, data, nonce, aicAmount)
      .sign(this.keypair);
  }

  /**
   * Accept a job as a provider
   * 
   * @param jobId - Job ID to accept
   */
  async acceptJob(jobId: Hash): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: acceptJob(bytes32 jobId)
    const data = this.encodeCall('acceptJob', [jobId]);
    
    return TransactionBuilder
      .call(this.keypair.address, JOB_ESCROW_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Submit job result with VCR
   * 
   * @param jobId - Job ID
   * @param result - Computation result
   * @param vcr - Verifiable Compute Receipt
   */
  async submitResult(
    jobId: Hash,
    result: Bytes,
    vcr: VerifiableComputeReceipt
  ): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: submitResult(bytes32 jobId, bytes result, VCR vcr)
    const data = this.encodeResultSubmission(jobId, result, vcr);
    
    return TransactionBuilder
      .call(this.keypair.address, JOB_ESCROW_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Challenge a job result
   * 
   * @param jobId - Job ID to challenge
   * @param challengeStake - Stake for challenge (slashed if challenge fails)
   */
  async challengeResult(jobId: Hash, challengeStake: bigint): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    if (challengeStake <= 0n) {
      throw new Error('Challenge stake must be positive');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: challengeResult(bytes32 jobId)
    const data = this.encodeCall('challengeResult', [jobId]);
    
    return TransactionBuilder
      .call(this.keypair.address, JOB_ESCROW_CONTRACT, data, nonce, challengeStake)
      .sign(this.keypair);
  }

  /**
   * Claim job payment (for provider after challenge period)
   * 
   * @param jobId - Job ID
   */
  async claimPayment(jobId: Hash): Promise<SignedTransaction> {
    if (!this.keypair) {
      throw new Error('Keypair required for signing transactions');
    }

    const nonce = await this.client.getNonce(this.keypair.address);
    
    // Encode call data: claimPayment(bytes32 jobId)
    const data = this.encodeCall('claimPayment', [jobId]);
    
    return TransactionBuilder
      .call(this.keypair.address, JOB_ESCROW_CONTRACT, data, nonce)
      .sign(this.keypair);
  }

  /**
   * Get VCR for a job
   */
  async getVCR(jobId: Hash): Promise<VerifiableComputeReceipt | null> {
    try {
      return await this.client['call']<VerifiableComputeReceipt>('ai_getVCR', [jobId]);
    } catch {
      return null;
    }
  }

  /**
   * Verify a VCR (check KZG commitments and TEE attestation)
   */
  async verifyVCR(vcr: VerifiableComputeReceipt): Promise<{
    valid: boolean;
    kzgValid: boolean;
    teeValid: boolean;
  }> {
    return this.client['call']('ai_verifyVCR', [vcr]);
  }

  /**
   * Wait for job completion
   * 
   * @param jobId - Job ID
   * @param timeout - Timeout in milliseconds (default: 5 minutes)
   * @param pollInterval - Poll interval in milliseconds (default: 2 seconds)
   */
  async waitForJobCompletion(
    jobId: Hash,
    timeout: number = 300000,
    pollInterval: number = 2000
  ): Promise<AIJob> {
    const startTime = Date.now();
    
    while (Date.now() - startTime < timeout) {
      const job = await this.getJob(jobId);
      
      if (!job) {
        throw new Error(`Job ${jobId} not found`);
      }
      
      if (job.status === 'completed' || job.status === 'settled') {
        return job;
      }
      
      if (job.status === 'challenged') {
        throw new Error(`Job ${jobId} is being challenged`);
      }
      
      await new Promise(resolve => setTimeout(resolve, pollInterval));
    }
    
    throw new Error(`Job ${jobId} did not complete within ${timeout}ms`);
  }

  /**
   * Get job statistics
   */
  async getJobStats(): Promise<{
    totalJobs: number;
    pendingJobs: number;
    completedJobs: number;
    challengedJobs: number;
    totalVolume: bigint;
  }> {
    return this.client['call']('ai_getJobStats', []);
  }

  /**
   * Get provider reputation
   */
  async getProviderReputation(provider: Address): Promise<{
    score: number;
    completedJobs: number;
    failedJobs: number;
    averageTime: number;
  }> {
    return this.client['call']('ai_getProviderReputation', [provider]);
  }

  /**
   * Encode job submission call data
   */
  private encodeJobSubmission(modelHash: Hash, inputData: Bytes): Uint8Array {
    // Simplified encoding - production would use proper ABI encoding
    const encoder = new TextEncoder();
    const method = encoder.encode('submitJob');
    const selector = method.slice(0, 4);
    
    // In practice, this would properly encode the modelHash and inputData
    return new Uint8Array([...selector, ...Buffer.from(modelHash.slice(2), 'hex'), ...inputData]);
  }

  /**
   * Encode result submission call data
   */
  private encodeResultSubmission(jobId: Hash, result: Bytes, vcr: VerifiableComputeReceipt): Uint8Array {
    // Simplified encoding
    const encoder = new TextEncoder();
    const method = encoder.encode('submitResult');
    const selector = method.slice(0, 4);
    
    return new Uint8Array([...selector, ...Buffer.from(jobId.slice(2), 'hex'), ...result]);
  }

  /**
   * Simple function selector encoding
   */
  private encodeCall(method: string, params: any[]): Uint8Array {
    const encoder = new TextEncoder();
    const signature = encoder.encode(method);
    
    const selector = new Uint8Array(4);
    const hash = encoder.encode(method + JSON.stringify(params));
    selector.set(hash.slice(0, 4));
    
    return selector;
  }
}

/**
 * Model registry helper
 */
export class ModelHelper {
  constructor(private client: AetherClient) {}

  /**
   * Register a model
   */
  async registerModel(
    modelHash: Hash,
    metadata: {
      name: string;
      version: string;
      description: string;
      inputSchema: any;
      outputSchema: any;
    }
  ): Promise<void> {
    await this.client['call']('ai_registerModel', [modelHash, metadata]);
  }

  /**
   * Get model metadata
   */
  async getModel(modelHash: Hash): Promise<{
    hash: Hash;
    name: string;
    version: string;
    description: string;
    registered: number;
    jobCount: number;
  } | null> {
    try {
      return await this.client['call']('ai_getModel', [modelHash]);
    } catch {
      return null;
    }
  }

  /**
   * List all registered models
   */
  async listModels(): Promise<Hash[]> {
    return this.client['call']<Hash[]>('ai_listModels', []);
  }
}

