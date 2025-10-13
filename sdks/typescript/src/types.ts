export interface ClientConfig {
  defaultFee: bigint;
  defaultGasLimit: number;
}

export const DEFAULT_CONFIG: ClientConfig = {
  defaultFee: 2_000_000n,
  defaultGasLimit: 500_000,
};

export interface TransferRequestPayload {
  recipient: string;
  amount: bigint;
  memo?: string;
}

export interface TransactionFields {
  nonce: number;
  sender: string;
  senderPublicKey: string;
  recipient: string;
  amount: bigint;
  fee: bigint;
  gasLimit: number;
  memo?: string;
  signature: string;
}

export interface JobRequest {
  jobId: string;
  modelHash: string;
  inputHash: string;
  maxFee: bigint;
  expiresAt: number;
  metadata?: Record<string, unknown>;
}

export interface JobSubmission {
  url: string;
  method: "POST";
  headers: Record<string, string>;
  body: JobRequest;
}

export interface SubmitResponse {
  txHash: string;
  accepted: boolean;
}
