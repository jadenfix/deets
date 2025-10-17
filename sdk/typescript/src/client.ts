/**
 * Aether RPC Client
 * 
 * Main client for interacting with Aether blockchain nodes.
 */

import axios, { AxiosInstance } from 'axios';
import {
  AetherConfig,
  RPCRequest,
  RPCResponse,
  Block,
  BlockWithTransactions,
  SignedTransaction,
  Account,
  Hash,
  Address,
} from './types';

export class AetherClient {
  private http: AxiosInstance;
  private requestId: number = 0;
  private config: Required<AetherConfig>;

  constructor(config: AetherConfig) {
    this.config = {
      rpcUrl: config.rpcUrl,
      chainId: config.chainId ?? 1,
      timeout: config.timeout ?? 30000,
    };

    this.http = axios.create({
      baseURL: this.config.rpcUrl,
      timeout: this.config.timeout,
      headers: {
        'Content-Type': 'application/json',
      },
    });
  }

  /**
   * Low-level RPC call
   */
  private async call<T>(method: string, params: any[] = []): Promise<T> {
    const request: RPCRequest = {
      jsonrpc: '2.0',
      method,
      params,
      id: ++this.requestId,
    };

    try {
      const response = await this.http.post<RPCResponse<T>>('/', request);
      
      if (response.data.error) {
        throw new Error(
          `RPC Error: ${response.data.error.message} (code: ${response.data.error.code})`
        );
      }

      return response.data.result as T;
    } catch (error: any) {
      if (error.response) {
        throw new Error(`HTTP ${error.response.status}: ${error.response.statusText}`);
      }
      throw error;
    }
  }

  /**
   * Get current chain slot number
   */
  async getSlot(): Promise<number> {
    return this.call<number>('getSlot');
  }

  /**
   * Get block by slot number
   */
  async getBlock(slot: number, includeTransactions: boolean = false): Promise<Block | BlockWithTransactions> {
    return this.call<Block | BlockWithTransactions>('getBlock', [slot, includeTransactions]);
  }

  /**
   * Get block by hash
   */
  async getBlockByHash(hash: Hash, includeTransactions: boolean = false): Promise<Block | BlockWithTransactions> {
    return this.call<Block | BlockWithTransactions>('getBlockByHash', [hash, includeTransactions]);
  }

  /**
   * Get latest finalized block
   */
  async getLatestBlock(): Promise<Block> {
    return this.call<Block>('getLatestBlock');
  }

  /**
   * Get transaction by hash
   */
  async getTransaction(hash: Hash): Promise<SignedTransaction | null> {
    return this.call<SignedTransaction | null>('getTransaction', [hash]);
  }

  /**
   * Get account information
   */
  async getAccount(address: Address): Promise<Account> {
    return this.call<Account>('getAccount', [address]);
  }

  /**
   * Get account balance
   */
  async getBalance(address: Address): Promise<bigint> {
    const account = await this.getAccount(address);
    return account.balance;
  }

  /**
   * Get account nonce
   */
  async getNonce(address: Address): Promise<number> {
    const account = await this.getAccount(address);
    return account.nonce;
  }

  /**
   * Send signed transaction
   */
  async sendTransaction(signedTx: SignedTransaction): Promise<Hash> {
    return this.call<Hash>('sendTransaction', [signedTx]);
  }

  /**
   * Send raw transaction (hex-encoded)
   */
  async sendRawTransaction(rawTx: string): Promise<Hash> {
    return this.call<Hash>('sendRawTransaction', [rawTx]);
  }

  /**
   * Get transaction receipt
   */
  async getTransactionReceipt(hash: Hash): Promise<{
    transactionHash: Hash;
    blockHash: Hash;
    blockSlot: number;
    from: Address;
    to: Address;
    status: 'success' | 'failed';
    gasUsed: bigint;
    logs: any[];
  } | null> {
    return this.call('getTransactionReceipt', [hash]);
  }

  /**
   * Estimate gas for transaction
   */
  async estimateGas(tx: Partial<SignedTransaction>): Promise<bigint> {
    return this.call<bigint>('estimateGas', [tx]);
  }

  /**
   * Get chain ID
   */
  getChainId(): number {
    return this.config.chainId;
  }

  /**
   * Get RPC URL
   */
  getRpcUrl(): string {
    return this.config.rpcUrl;
  }

  /**
   * Check node health
   */
  async isHealthy(): Promise<boolean> {
    try {
      await this.getSlot();
      return true;
    } catch {
      return false;
    }
  }

  /**
   * Wait for transaction confirmation
   */
  async waitForTransaction(
    hash: Hash,
    timeout: number = 30000,
    pollInterval: number = 1000
  ): Promise<{
    transactionHash: Hash;
    blockHash: Hash;
    blockSlot: number;
    status: 'success' | 'failed';
  }> {
    const startTime = Date.now();
    
    while (Date.now() - startTime < timeout) {
      const receipt = await this.getTransactionReceipt(hash);
      
      if (receipt) {
        return {
          transactionHash: receipt.transactionHash,
          blockHash: receipt.blockHash,
          blockSlot: receipt.blockSlot,
          status: receipt.status,
        };
      }
      
      await new Promise(resolve => setTimeout(resolve, pollInterval));
    }
    
    throw new Error(`Transaction ${hash} not confirmed within ${timeout}ms`);
  }
}

