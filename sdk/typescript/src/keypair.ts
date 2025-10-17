/**
 * Keypair management for Aether
 * 
 * Ed25519 keypair generation and signing
 */

import * as ed25519 from '@noble/ed25519';
import { sha256 } from '@noble/hashes/sha256';
import { Keypair, Bytes, Address, Signature } from './types';

export class AetherKeypair implements Keypair {
  public readonly publicKey: Bytes;
  public readonly secretKey: Bytes;
  public readonly address: Address;

  private constructor(secretKey: Bytes, publicKey: Bytes) {
    this.secretKey = secretKey;
    this.publicKey = publicKey;
    this.address = AetherKeypair.publicKeyToAddress(publicKey);
  }

  /**
   * Generate a new random keypair
   */
  static async generate(): Promise<AetherKeypair> {
    const secretKey = ed25519.utils.randomPrivateKey();
    const publicKey = await ed25519.getPublicKey(secretKey);
    return new AetherKeypair(secretKey, publicKey);
  }

  /**
   * Create keypair from existing secret key
   */
  static async fromSecretKey(secretKey: Bytes): Promise<AetherKeypair> {
    const publicKey = await ed25519.getPublicKey(secretKey);
    return new AetherKeypair(secretKey, publicKey);
  }

  /**
   * Create keypair from seed phrase (deterministic)
   */
  static async fromSeed(seed: string): Promise<AetherKeypair> {
    const seedBytes = new TextEncoder().encode(seed);
    const secretKey = sha256(seedBytes);
    return AetherKeypair.fromSecretKey(secretKey);
  }

  /**
   * Sign a message
   */
  async sign(message: Bytes): Promise<Signature> {
    const signature = await ed25519.sign(message, this.secretKey);
    return Buffer.from(signature).toString('hex');
  }

  /**
   * Verify a signature
   */
  static async verify(signature: Signature, message: Bytes, publicKey: Bytes): Promise<boolean> {
    try {
      const sigBytes = Buffer.from(signature, 'hex');
      return await ed25519.verify(sigBytes, message, publicKey);
    } catch {
      return false;
    }
  }

  /**
   * Convert public key to Aether address
   */
  static publicKeyToAddress(publicKey: Bytes): Address {
    const hash = sha256(publicKey);
    // Take last 20 bytes of hash
    const addressBytes = hash.slice(-20);
    return '0x' + Buffer.from(addressBytes).toString('hex');
  }

  /**
   * Export secret key as hex string
   */
  toSecretKeyHex(): string {
    return Buffer.from(this.secretKey).toString('hex');
  }

  /**
   * Export public key as hex string
   */
  toPublicKeyHex(): string {
    return Buffer.from(this.publicKey).toString('hex');
  }

  /**
   * Create keypair from hex-encoded secret key
   */
  static async fromSecretKeyHex(hex: string): Promise<AetherKeypair> {
    const secretKey = Buffer.from(hex, 'hex');
    return AetherKeypair.fromSecretKey(secretKey);
  }
}

