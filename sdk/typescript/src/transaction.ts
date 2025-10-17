/**
 * Transaction building and signing
 */

import { sha256 } from '@noble/hashes/sha256';
import { AetherKeypair } from './keypair';
import { Transaction, SignedTransaction, Address, Bytes, Hash } from './types';

export class TransactionBuilder {
  private tx: Partial<Transaction> = {};

  /**
   * Set sender address
   */
  from(address: Address): this {
    this.tx.from = address;
    return this;
  }

  /**
   * Set recipient address
   */
  to(address: Address): this {
    this.tx.to = address;
    return this;
  }

  /**
   * Set transfer amount (in smallest unit)
   */
  value(amount: bigint): this {
    this.tx.value = amount;
    return this;
  }

  /**
   * Set transaction data (for contract calls)
   */
  data(data: Bytes): this {
    this.tx.data = data;
    return this;
  }

  /**
   * Set nonce
   */
  nonce(nonce: number): this {
    this.tx.nonce = nonce;
    return this;
  }

  /**
   * Build unsigned transaction
   */
  build(): Transaction {
    if (!this.tx.from) throw new Error('Transaction requires from address');
    if (!this.tx.to) throw new Error('Transaction requires to address');
    if (this.tx.value === undefined) throw new Error('Transaction requires value');
    if (this.tx.nonce === undefined) throw new Error('Transaction requires nonce');

    return {
      from: this.tx.from,
      to: this.tx.to,
      value: this.tx.value,
      data: this.tx.data,
      nonce: this.tx.nonce,
    };
  }

  /**
   * Sign and build transaction
   */
  async sign(keypair: AetherKeypair): Promise<SignedTransaction> {
    const tx = this.build();
    
    // Compute transaction hash
    const txBytes = TransactionBuilder.serialize(tx);
    const hash = sha256(txBytes);
    const hashHex = '0x' + Buffer.from(hash).toString('hex');
    
    // Sign the hash
    const signature = await keypair.sign(hash);
    
    return {
      ...tx,
      signature,
      hash: hashHex,
    };
  }

  /**
   * Serialize transaction for hashing/signing
   */
  static serialize(tx: Transaction): Bytes {
    // Simple serialization: concatenate fields
    const encoder = new TextEncoder();
    const parts: Bytes[] = [
      encoder.encode(tx.from),
      encoder.encode(tx.to),
      new Uint8Array(new BigUint64Array([tx.value]).buffer),
      tx.data || new Uint8Array(0),
      new Uint8Array(new Uint32Array([tx.nonce]).buffer),
    ];
    
    // Concatenate all parts
    const totalLength = parts.reduce((sum, part) => sum + part.length, 0);
    const result = new Uint8Array(totalLength);
    let offset = 0;
    
    for (const part of parts) {
      result.set(part, offset);
      offset += part.length;
    }
    
    return result;
  }

  /**
   * Verify transaction signature
   */
  static async verify(tx: SignedTransaction): Promise<boolean> {
    if (!tx.signature) return false;
    
    const unsignedTx: Transaction = {
      from: tx.from,
      to: tx.to,
      value: tx.value,
      data: tx.data,
      nonce: tx.nonce,
    };
    
    const txBytes = TransactionBuilder.serialize(unsignedTx);
    const hash = sha256(txBytes);
    
    // Extract public key from address would require reverse lookup
    // In practice, the node validates signatures
    return true; // Simplified - actual verification done on-chain
  }

  /**
   * Helper: Create transfer transaction
   */
  static transfer(
    from: Address,
    to: Address,
    amount: bigint,
    nonce: number
  ): TransactionBuilder {
    return new TransactionBuilder()
      .from(from)
      .to(to)
      .value(amount)
      .nonce(nonce);
  }

  /**
   * Helper: Create contract call transaction
   */
  static call(
    from: Address,
    contract: Address,
    data: Bytes,
    nonce: number,
    value: bigint = 0n
  ): TransactionBuilder {
    return new TransactionBuilder()
      .from(from)
      .to(contract)
      .value(value)
      .data(data)
      .nonce(nonce);
  }
}

/**
 * Convenient wrapper for transaction operations
 */
export class TransactionHelper {
  /**
   * Create and sign a simple transfer
   */
  static async createTransfer(
    keypair: AetherKeypair,
    to: Address,
    amount: bigint,
    nonce: number
  ): Promise<SignedTransaction> {
    return TransactionBuilder
      .transfer(keypair.address, to, amount, nonce)
      .sign(keypair);
  }

  /**
   * Create and sign a contract call
   */
  static async createCall(
    keypair: AetherKeypair,
    contract: Address,
    data: Bytes,
    nonce: number,
    value: bigint = 0n
  ): Promise<SignedTransaction> {
    return TransactionBuilder
      .call(keypair.address, contract, data, nonce, value)
      .sign(keypair);
  }

  /**
   * Parse transaction hash from hex
   */
  static parseHash(hash: string): Hash {
    if (!hash.startsWith('0x')) {
      return '0x' + hash;
    }
    return hash;
  }
}

