/**
 * Example 4: AI Job Submission and Tracking
 * 
 * Demonstrates:
 * - Submitting an AI inference job
 * - Tracking job status
 * - Verifying results
 * - Handling VCR
 */

import { AetherClient, AetherKeypair, AIJobHelper } from '@aether/sdk';

async function main() {
  const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });
  const keypair = await AetherKeypair.fromSeed('my seed phrase');

  const ai = new AIJobHelper(client, keypair);

  const modelHash = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
  
  const inputData = new TextEncoder().encode(JSON.stringify({
    prompt: 'Write a haiku about blockchain',
    temperature: 0.7,
    maxTokens: 50,
  }));

  console.log('Submitting AI job...');
  const submitTx = await ai.submitJob(
    modelHash,
    inputData,
    100000n
  );

  const txHash = await client.sendTransaction(submitTx);
  console.log('Job submitted:', txHash);

  const receipt = await client.waitForTransaction(txHash);
  console.log('Transaction confirmed in slot:', receipt.blockSlot);

  const jobId = submitTx.hash;
  console.log('Job ID:', jobId);

  console.log('\nWaiting for provider to accept and complete job...');
  
  let job = await ai.getJob(jobId);
  console.log('Initial status:', job?.status);

  try {
    job = await ai.waitForJobCompletion(jobId, 120000);
    console.log('\nJob completed!');
    console.log('- Status:', job.status);
    console.log('- Provider:', job.provider);
    
    if (job.result) {
      const resultText = new TextDecoder().decode(job.result);
      console.log('- Result:', resultText);
    }

    if (job.vcr) {
      console.log('\nVerifying Compute Receipt...');
      const verification = await ai.verifyVCR(job.vcr);
      console.log('- Valid:', verification.valid);
      console.log('- KZG Proof Valid:', verification.kzgValid);
      console.log('- TEE Attestation Valid:', verification.teeValid);

      if (verification.valid) {
        console.log('\nResult is cryptographically verified!');
      } else {
        console.log('\nWarning: Invalid VCR detected!');
        console.log('Consider challenging this result');
      }
    }

    const reputation = await ai.getProviderReputation(job.provider!);
    console.log('\nProvider Reputation:');
    console.log('- Score:', reputation.score);
    console.log('- Completed Jobs:', reputation.completedJobs);
    console.log('- Average Time:', reputation.averageTime, 'seconds');

  } catch (error) {
    console.error('Job failed or timed out:', error);
    
    job = await ai.getJob(jobId);
    if (job?.status === 'pending') {
      console.log('No providers available. Run an AI provider node.');
    }
  }
}

main().catch(console.error);

