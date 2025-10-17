/**
 * Core types for Aether SDK
 */

export type Bytes = Uint8Array;
export type Address = string;
export type Hash = string;
export type Signature = string;

/**
 * Transaction types
 */
export interface Transaction {
  from: Address;
  to: Address;
  value: bigint;
  data?: Bytes;
  nonce: number;
  signature?: Signature;
}

export interface SignedTransaction extends Transaction {
  signature: Signature;
  hash: Hash;
}

/**
 * Block information
 */
export interface Block {
  slot: number;
  hash: Hash;
  parentHash: Hash;
  proposer: Address;
  transactions: Hash[];
  stateRoot: Hash;
  timestamp: number;
  vrfProof?: Bytes;
}

export interface BlockWithTransactions extends Block {
  transactions: SignedTransaction[];
}

/**
 * Account information
 */
export interface Account {
  address: Address;
  balance: bigint;
  nonce: number;
  codeHash?: Hash;
}

/**
 * Staking types
 */
export interface Validator {
  address: Address;
  stake: bigint;
  delegatedStake: bigint;
  commission: number; // basis points (0-10000)
  active: boolean;
  uptime: number;
}

export interface Delegation {
  delegator: Address;
  validator: Address;
  amount: bigint;
  rewards: bigint;
}

/**
 * Governance types
 */
export interface Proposal {
  id: number;
  proposer: Address;
  title: string;
  description: string;
  votesFor: bigint;
  votesAgainst: bigint;
  status: 'active' | 'passed' | 'rejected' | 'executed';
  startSlot: number;
  endSlot: number;
}

export interface Vote {
  proposalId: number;
  voter: Address;
  support: boolean;
  votingPower: bigint;
}

/**
 * AI Job types
 */
export interface AIJob {
  id: Hash;
  creator: Address;
  modelHash: Hash;
  inputData: Bytes;
  aicLocked: bigint;
  status: 'pending' | 'assigned' | 'computing' | 'completed' | 'challenged' | 'settled';
  provider?: Address;
  result?: Bytes;
  vcr?: VerifiableComputeReceipt;
}

export interface VerifiableComputeReceipt {
  jobId: Hash;
  provider: Address;
  result: Bytes;
  executionTrace: Hash;
  kzgCommitments: Bytes[];
  teeAttestation: Bytes;
  timestamp: number;
}

/**
 * RPC request/response types
 */
export interface RPCRequest {
  jsonrpc: '2.0';
  method: string;
  params?: any[];
  id: number;
}

export interface RPCResponse<T = any> {
  jsonrpc: '2.0';
  result?: T;
  error?: {
    code: number;
    message: string;
    data?: any;
  };
  id: number;
}

/**
 * Configuration
 */
export interface AetherConfig {
  rpcUrl: string;
  chainId?: number;
  timeout?: number;
}

/**
 * Keypair for signing
 */
export interface Keypair {
  publicKey: Bytes;
  secretKey: Bytes;
  address: Address;
}

