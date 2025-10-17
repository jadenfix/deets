/**
 * Example 1: Basic AIC Token Transfer
 * 
 * Demonstrates:
 * - Keypair generation
 * - Balance checking
 * - Simple transfer
 * - Transaction confirmation
 */

import { AetherClient, AetherKeypair, TransactionHelper } from '@aether/sdk';

async function main() {
  const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });

  const sender = await AetherKeypair.fromSeed('sender seed phrase');
  const recipient = await AetherKeypair.generate();

  console.log('Sender:', sender.address);
  console.log('Recipient:', recipient.address);

  const balance = await client.getBalance(sender.address);
  console.log('Sender balance:', balance.toString(), 'AIC');

  if (balance < 1000n) {
    throw new Error('Insufficient balance');
  }

  const nonce = await client.getNonce(sender.address);

  const tx = await TransactionHelper.createTransfer(
    sender,
    recipient.address,
    1000n,
    nonce
  );

  console.log('Sending transaction...');
  const txHash = await client.sendTransaction(tx);
  console.log('Transaction hash:', txHash);

  const receipt = await client.waitForTransaction(txHash);
  console.log('Confirmed in slot:', receipt.blockSlot);
  console.log('Status:', receipt.status);

  const newBalance = await client.getBalance(recipient.address);
  console.log('Recipient new balance:', newBalance.toString(), 'AIC');
}

main().catch(console.error);

