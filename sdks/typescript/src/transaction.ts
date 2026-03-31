import { createHash } from "node:crypto";

import type { TransactionFields } from "./types.js";

export class Transaction {
  readonly nonce: number;
  readonly sender: string;
  readonly senderPublicKey: string;
  readonly recipient: string;
  readonly amount: bigint;
  readonly fee: bigint;
  readonly gasLimit: number;
  readonly memo?: string;
  readonly signature: string;
  readonly reads: string[];
  readonly writes: string[];

  constructor(fields: TransactionFields) {
    if (!fields.signature || fields.signature.length !== 128) {
      throw new Error("signature must be exactly 64 bytes (128 hex characters)");
    }
    if (!/^[0-9a-fA-F]+$/.test(fields.signature)) {
      throw new Error("signature must be valid hex");
    }

    this.nonce = fields.nonce;
    this.sender = fields.sender;
    this.senderPublicKey = fields.senderPublicKey;
    this.recipient = fields.recipient;
    this.amount = fields.amount;
    this.fee = fields.fee;
    this.gasLimit = fields.gasLimit;
    this.memo = fields.memo;
    this.signature = fields.signature;
    this.reads = [fields.sender];
    this.writes =
      fields.sender === fields.recipient
        ? [fields.sender]
        : [fields.sender, fields.recipient];
  }

  hash(): string {
    const forHash = {
      nonce: this.nonce,
      sender: this.sender,
      senderPublicKey: this.senderPublicKey,
      recipient: this.recipient,
      amount: this.amount.toString(),
      fee: this.fee.toString(),
      gasLimit: this.gasLimit,
      memo: this.memo,
      reads: this.reads,
      writes: this.writes,
    };
    // Sort keys for deterministic serialization (matches Python SDK's sort_keys=True)
    const sortedKeys = Object.keys(forHash).sort();
    const sorted: Record<string, unknown> = {};
    for (const key of sortedKeys) {
      sorted[key] = (forHash as Record<string, unknown>)[key];
    }
    const hash = createHash("sha256")
      .update(JSON.stringify(sorted))
      .digest("hex");
    return `0x${hash}`;
  }

  toJSON() {
    return {
      nonce: this.nonce,
      sender: this.sender,
      senderPublicKey: this.senderPublicKey,
      recipient: this.recipient,
      amount: this.amount.toString(),
      fee: this.fee.toString(),
      gasLimit: this.gasLimit,
      memo: this.memo,
      signature: this.signature,
      reads: this.reads,
      writes: this.writes,
    };
  }

  toRpcTransaction() {
    return {
      nonce: this.nonce,
      sender: this.sender,
      sender_public_key: this.senderPublicKey,
      recipient: this.recipient,
      amount: this.amount.toString(),
      fee: this.fee.toString(),
      gas_limit: this.gasLimit,
      memo: this.memo,
      reads: this.reads,
      writes: this.writes,
      signature: this.signature,
    };
  }
}
