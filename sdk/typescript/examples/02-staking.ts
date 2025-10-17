/**
 * Example 2: Staking and Delegation
 * 
 * Demonstrates:
 * - Viewing validators
 * - Delegating stake
 * - Checking rewards
 * - Claiming rewards
 */

import { AetherClient, AetherKeypair, StakingHelper } from '@aether/sdk';

async function main() {
  const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });
  const keypair = await AetherKeypair.fromSeed('my seed phrase');

  const staking = new StakingHelper(client, keypair);

  console.log('Fetching validators...');
  const validators = await staking.getValidators();
  
  console.log(`Found ${validators.length} active validators`);
  for (const v of validators.slice(0, 5)) {
    console.log(`- ${v.address}: ${v.stake} stake, ${v.commission/100}% commission`);
  }

  const bestValidator = validators.sort((a, b) => 
    (b.uptime - a.uptime) || (a.commission - b.commission)
  )[0];

  console.log(`\nDelegating to best validator: ${bestValidator.address}`);
  
  const delegateTx = await staking.delegate(bestValidator.address, 10000n);
  const txHash = await client.sendTransaction(delegateTx);
  console.log('Delegation tx:', txHash);
  
  await client.waitForTransaction(txHash);
  console.log('Delegation confirmed');

  console.log('\nChecking pending rewards...');
  const rewards = await staking.getPendingRewards(keypair.address);
  console.log('Pending rewards:', rewards.toString(), 'AIC');

  if (rewards > 0n) {
    console.log('Claiming rewards...');
    const claimTx = await staking.claimRewards();
    const claimHash = await client.sendTransaction(claimTx);
    await client.waitForTransaction(claimHash);
    console.log('Rewards claimed!');
  }

  const delegation = await staking.getDelegation(keypair.address, bestValidator.address);
  console.log('\nDelegation info:');
  console.log('- Amount:', delegation?.amount.toString());
  console.log('- Rewards:', delegation?.rewards.toString());
}

main().catch(console.error);

