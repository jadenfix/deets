import { ClientConfig, DEFAULT_CONFIG, JobRequest, JobSubmission } from "./types.js";
import { Transaction } from "./transaction.js";

export class TransferBuilder {
  private _recipient?: string;
  private _amount?: bigint;
  private _memo?: string;
  private _fee: bigint;
  private _gasLimit: number;

  constructor(private readonly config: ClientConfig = DEFAULT_CONFIG) {
    this._fee = config.defaultFee;
    this._gasLimit = config.defaultGasLimit;
  }

  to(recipient: string): TransferBuilder {
    if (!recipient.startsWith("0x")) {
      throw new Error("recipient must be a hex string");
    }
    this._recipient = recipient;
    return this;
  }

  amount(amount: bigint | number | string): TransferBuilder {
    const value =
      typeof amount === "bigint" ? amount : BigInt(amount.toString());
    if (value <= 0n) {
      throw new Error("amount must be positive");
    }
    this._amount = value;
    return this;
  }

  memo(memo: string): TransferBuilder {
    this._memo = memo;
    return this;
  }

  fee(fee: bigint | number | string): TransferBuilder {
    const value =
      typeof fee === "bigint" ? fee : BigInt(fee.toString());
    if (value <= 0n) {
      throw new Error("fee must be positive");
    }
    this._fee = value;
    return this;
  }

  gasLimit(gasLimit: number): TransferBuilder {
    if (gasLimit <= 0) {
      throw new Error("gasLimit must be positive");
    }
    this._gasLimit = gasLimit;
    return this;
  }

  build(options: {
    sender: string;
    senderPublicKey: string;
    signature: string;
    nonce: number;
  }): Transaction {
    if (!this._recipient) {
      throw new Error("recipient not set");
    }
    if (!this._amount) {
      throw new Error("amount not set");
    }
    if (!options.sender.startsWith("0x")) {
      throw new Error("sender must be a hex string");
    }
    if (options.nonce < 0) {
      throw new Error("nonce must be non-negative");
    }
    if (options.signature.length < 64) {
      throw new Error("signature must be 64+ hex chars");
    }

    return new Transaction({
      nonce: options.nonce,
      sender: options.sender,
      senderPublicKey: options.senderPublicKey,
      recipient: this._recipient,
      amount: this._amount,
      fee: this._fee,
      gasLimit: this._gasLimit,
      memo: this._memo,
      signature: options.signature,
    });
  }
}

export class JobBuilder {
  private _jobId?: string;
  private _modelHash?: string;
  private _inputHash?: string;
  private _maxFee: bigint;
  private _expiresAt?: number;
  private _metadata?: Record<string, unknown>;

  constructor(private readonly endpoint: string) {
    this._maxFee = 1_000_000n;
  }

  id(jobId: string): JobBuilder {
    if (!jobId.trim()) {
      throw new Error("jobId must not be empty");
    }
    this._jobId = jobId;
    return this;
  }

  model(hash: string): JobBuilder {
    if (!hash.startsWith("0x")) {
      throw new Error("model hash must be hex");
    }
    this._modelHash = hash;
    return this;
  }

  input(hash: string): JobBuilder {
    if (!hash.startsWith("0x")) {
      throw new Error("input hash must be hex");
    }
    this._inputHash = hash;
    return this;
  }

  maxFee(fee: bigint | number | string): JobBuilder {
    const value =
      typeof fee === "bigint" ? fee : BigInt(fee.toString());
    if (value <= 0n) {
      throw new Error("max fee must be positive");
    }
    this._maxFee = value;
    return this;
  }

  expiresAt(timestamp: number | Date): JobBuilder {
    const value =
      timestamp instanceof Date ? Math.floor(timestamp.getTime() / 1000) : timestamp;
    if (value <= 0) {
      throw new Error("expiry must be in the future");
    }
    this._expiresAt = value;
    return this;
  }

  withMetadata(metadata: Record<string, unknown>): JobBuilder {
    this._metadata = metadata;
    return this;
  }

  build(): JobRequest {
    if (!this._jobId) {
      throw new Error("jobId not set");
    }
    if (!this._modelHash) {
      throw new Error("model hash not set");
    }
    if (!this._inputHash) {
      throw new Error("input hash not set");
    }
    if (!this._expiresAt) {
      throw new Error("expiry not set");
    }

    return {
      jobId: this._jobId,
      modelHash: this._modelHash,
      inputHash: this._inputHash,
      maxFee: this._maxFee,
      expiresAt: this._expiresAt,
      metadata: this._metadata,
    };
  }

  toSubmission(): JobSubmission {
    const job = this.build();
    return {
      url: `${this.endpoint.replace(/\/+$/, "")}/v1/jobs`,
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: job,
    };
  }
}
