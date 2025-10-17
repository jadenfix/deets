/**
 * Example 5: Batch AI Job Processing
 * 
 * Demonstrates:
 * - Submitting multiple jobs in parallel
 * - Tracking multiple jobs
 * - Aggregating results
 */

import { AetherClient, AetherKeypair, AIJobHelper } from '@aether/sdk';

async function main() {
  const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });
  const keypair = await AetherKeypair.fromSeed('my seed phrase');

  const ai = new AIJobHelper(client, keypair);

  const modelHash = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';

  const prompts = [
    'Summarize blockchain technology',
    'Explain smart contracts',
    'What is proof of stake?',
    'Describe consensus mechanisms',
    'What are verifiable compute receipts?',
  ];

  console.log(`Submitting ${prompts.length} jobs in parallel...`);

  const jobPromises = prompts.map(async (prompt, i) => {
    const inputData = new TextEncoder().encode(JSON.stringify({ prompt }));
    
    const nonce = await client.getNonce(keypair.address);
    const tx = await ai.submitJob(modelHash, inputData, 50000n);
    
    const txHash = await client.sendTransaction(tx);
    console.log(`Job ${i + 1} submitted:`, txHash);
    
    await client.waitForTransaction(txHash);
    return { jobId: tx.hash, prompt, index: i };
  });

  const jobs = await Promise.all(jobPromises);
  console.log(`\nAll ${jobs.length} jobs submitted!`);

  console.log('\nWaiting for completions...');
  
  const completionPromises = jobs.map(async ({ jobId, prompt, index }) => {
    try {
      const job = await ai.waitForJobCompletion(jobId, 180000);
      
      const result = job.result ? new TextDecoder().decode(job.result) : null;
      
      return {
        index,
        prompt,
        success: true,
        result,
        provider: job.provider,
      };
    } catch (error) {
      return {
        index,
        prompt,
        success: false,
        error: error.message,
      };
    }
  });

  const results = await Promise.all(completionPromises);

  console.log('\n=== Results ===\n');
  
  const successful = results.filter(r => r.success);
  const failed = results.filter(r => !r.success);

  for (const result of successful) {
    console.log(`Job ${result.index + 1}: ${result.prompt}`);
    console.log(`Provider: ${result.provider}`);
    console.log(`Result: ${result.result}`);
    console.log();
  }

  console.log(`\nSummary: ${successful.length}/${results.length} jobs completed`);
  
  if (failed.length > 0) {
    console.log('\nFailed jobs:');
    for (const result of failed) {
      console.log(`- Job ${result.index + 1}: ${result.error}`);
    }
  }

  const stats = await ai.getJobStats();
  console.log('\nNetwork Statistics:');
  console.log('- Total Jobs:', stats.totalJobs);
  console.log('- Pending Jobs:', stats.pendingJobs);
  console.log('- Completed Jobs:', stats.completedJobs);
  console.log('- Total Volume:', stats.totalVolume.toString(), 'AIC');
}

main().catch(console.error);

