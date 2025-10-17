/**
 * Integration Tests for Aether TypeScript SDK
 * 
 * Tests end-to-end developer workflows
 */

import {
  AetherClient,
  AetherKeypair,
  TransactionHelper,
  StakingHelper,
  GovernanceHelper,
  AIJobHelper,
} from '../index';

describe('Aether SDK Integration Tests', () => {
  let client: AetherClient;
  let keypair: AetherKeypair;

  beforeAll(async () => {
    client = new AetherClient({ rpcUrl: 'http://localhost:8545' });
    keypair = await AetherKeypair.fromSeed('test-seed-phrase');
  });

  describe('Core Client', () => {
    test('should connect to node', async () => {
      const healthy = await client.isHealthy();
      expect(healthy).toBe(true);
    });

    test('should get current slot', async () => {
      const slot = await client.getSlot();
      expect(typeof slot).toBe('number');
      expect(slot).toBeGreaterThan(0);
    });

    test('should get account balance', async () => {
      const balance = await client.getBalance(keypair.address);
      expect(typeof balance).toBe('bigint');
      expect(balance).toBeGreaterThanOrEqual(0n);
    });
  });

  describe('Keypair Management', () => {
    test('should generate new keypair', async () => {
      const newKeypair = await AetherKeypair.generate();
      expect(newKeypair.address).toMatch(/^0x[a-fA-F0-9]{40}$/);
      expect(newKeypair.publicKey).toBeInstanceOf(Uint8Array);
      expect(newKeypair.secretKey).toBeInstanceOf(Uint8Array);
    });

    test('should create keypair from seed', async () => {
      const keypair1 = await AetherKeypair.fromSeed('test');
      const keypair2 = await AetherKeypair.fromSeed('test');
      expect(keypair1.address).toBe(keypair2.address);
    });

    test('should sign and verify message', async () => {
      const message = new TextEncoder().encode('Hello Aether');
      const signature = await keypair.sign(message);
      
      const valid = await AetherKeypair.verify(
        signature,
        message,
        keypair.publicKey
      );
      expect(valid).toBe(true);
    });
  });

  describe('Transaction Building', () => {
    test('should build unsigned transaction', async () => {
      const nonce = await client.getNonce(keypair.address);
      
      const tx = await TransactionHelper.createTransfer(
        keypair,
        '0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb',
        1000n,
        nonce
      );

      expect(tx.from).toBe(keypair.address);
      expect(tx.to).toBe('0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb');
      expect(tx.value).toBe(1000n);
      expect(tx.signature).toBeDefined();
      expect(tx.hash).toBeDefined();
    });
  });

  describe('Staking Operations', () => {
    test('should fetch validators', async () => {
      const staking = new StakingHelper(client);
      const validators = await staking.getValidators();
      
      expect(Array.isArray(validators)).toBe(true);
      if (validators.length > 0) {
        const v = validators[0];
        expect(v.address).toMatch(/^0x[a-fA-F0-9]{40}$/);
        expect(typeof v.stake).toBe('bigint');
        expect(typeof v.commission).toBe('number');
      }
    });

    test('should get total stake', async () => {
      const staking = new StakingHelper(client);
      const totalStake = await staking.getTotalStake();
      expect(typeof totalStake).toBe('bigint');
      expect(totalStake).toBeGreaterThanOrEqual(0n);
    });
  });

  describe('Governance Operations', () => {
    test('should fetch active proposals', async () => {
      const gov = new GovernanceHelper(client);
      const proposals = await gov.getActiveProposals();
      
      expect(Array.isArray(proposals)).toBe(true);
    });

    test('should get voting power', async () => {
      const gov = new GovernanceHelper(client);
      const power = await gov.getVotingPower(keypair.address);
      expect(typeof power).toBe('bigint');
      expect(power).toBeGreaterThanOrEqual(0n);
    });
  });

  describe('AI Job Operations', () => {
    test('should get pending jobs', async () => {
      const ai = new AIJobHelper(client);
      const jobs = await ai.getPendingJobs();
      
      expect(Array.isArray(jobs)).toBe(true);
    });

    test('should get job stats', async () => {
      const ai = new AIJobHelper(client);
      const stats = await ai.getJobStats();
      
      expect(typeof stats.totalJobs).toBe('number');
      expect(typeof stats.completedJobs).toBe('number');
      expect(typeof stats.totalVolume).toBe('bigint');
    });
  });

  describe('Error Handling', () => {
    test('should handle invalid address', async () => {
      await expect(
        client.getBalance('invalid')
      ).rejects.toThrow();
    });

    test('should handle non-existent transaction', async () => {
      const tx = await client.getTransaction('0x0000000000000000000000000000000000000000000000000000000000000000');
      expect(tx).toBeNull();
    });
  });
});

