# Hello AIC Job: 10-Minute Quickstart

Submit your first verifiable AI compute job on Aether in under 10 minutes.

## Prerequisites

- Node.js 18+ or Python 3.8+
- A running Aether node (local devnet or testnet access)
- Basic understanding of blockchain transactions

## What You'll Build

A simple application that:
1. Connects to Aether
2. Creates a keypair
3. Funds the account (from faucet)
4. Submits an AI inference job
5. Tracks the job to completion
6. Verifies the result

Estimated time: 8 minutes

## TypeScript Version

### Step 1: Setup (1 minute)

```bash
mkdir aether-hello-job
cd aether-hello-job
npm init -y
npm install @aether/sdk
```

### Step 2: Write the Code (3 minutes)

Create `index.ts`:

```typescript
import { AetherClient, AetherKeypair, AIJobHelper } from '@aether/sdk';

async function main() {
  // 1. Connect to Aether devnet
  const client = new AetherClient({ 
    rpcUrl: 'http://localhost:8545',
    chainId: 1 
  });

  // 2. Generate keypair
  const keypair = await AetherKeypair.generate();
  console.log('Your address:', keypair.address);
  console.log('Secret key:', keypair.toSecretKeyHex());

  // 3. Request tokens from faucet
  console.log('Request test tokens from: http://localhost:9000/faucet');
  console.log('Paste your address:', keypair.address);
  await new Promise(resolve => setTimeout(resolve, 5000));

  // 4. Check balance
  const balance = await client.getBalance(keypair.address);
  console.log('Balance:', balance.toString(), 'AIC');

  if (balance < 1000000n) {
    throw new Error('Insufficient balance. Please fund from faucet.');
  }

  // 5. Submit AI job
  console.log('Submitting AI inference job...');
  const aiHelper = new AIJobHelper(client, keypair);

  const modelHash = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
  const inputData = new TextEncoder().encode(JSON.stringify({
    prompt: 'Explain blockchain in one sentence',
    temperature: 0.7,
  }));

  const tx = await aiHelper.submitJob(
    modelHash,
    inputData,
    100000n // 0.1 AIC
  );

  // 6. Send transaction
  const txHash = await client.sendTransaction(tx);
  console.log('Job submitted! Transaction:', txHash);

  // 7. Wait for confirmation
  const receipt = await client.waitForTransaction(txHash);
  console.log('Transaction confirmed in slot:', receipt.blockSlot);

  // Extract job ID from transaction data
  const jobId = tx.hash; // Simplified - actual jobId extracted from logs

  // 8. Wait for job completion
  console.log('Waiting for AI provider to complete job...');
  const job = await aiHelper.waitForJobCompletion(jobId, 60000);

  // 9. Get result
  console.log('Job completed!');
  console.log('Status:', job.status);
  console.log('Provider:', job.provider);
  
  if (job.result) {
    const resultText = new TextDecoder().decode(job.result);
    console.log('Result:', resultText);
  }

  // 10. Verify VCR
  if (job.vcr) {
    console.log('Verifying compute receipt...');
    const verification = await aiHelper.verifyVCR(job.vcr);
    console.log('KZG proof valid:', verification.kzgValid);
    console.log('TEE attestation valid:', verification.teeValid);
  }

  console.log('Success! You just ran verifiable AI compute on Aether.');
}

main().catch(console.error);
```

### Step 3: Run (2 minutes)

```bash
npx ts-node index.ts
```

Expected output:
```
Your address: 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb
Request test tokens from: http://localhost:9000/faucet
Balance: 1000000 AIC
Submitting AI inference job...
Job submitted! Transaction: 0xabc123...
Transaction confirmed in slot: 12345
Waiting for AI provider to complete job...
Job completed!
Status: completed
Provider: 0x8f3CF7ad51Df4F93Ecb0F5B1440cF78e03A1F99D
Result: {"output": "Blockchain is a distributed ledger..."}
KZG proof valid: true
TEE attestation valid: true
Success! You just ran verifiable AI compute on Aether.
```

## Python Version

### Step 1: Setup (1 minute)

```bash
mkdir aether-hello-job
cd aether-hello-job
python -m venv venv
source venv/bin/activate  # Windows: venv\Scripts\activate
pip install aether-sdk
```

### Step 2: Write the Code (3 minutes)

Create `main.py`:

```python
import asyncio
import json
from aether import AetherClient, Keypair, AIJobHelper

async def main():
    async with AetherClient(
        rpc_url="http://localhost:8545",
        chain_id=1
    ) as client:
        
        keypair = Keypair.generate()
        print(f"Your address: {keypair.address}")
        print(f"Secret key: {keypair.to_secret_key_hex()}")
        
        print("Request test tokens from: http://localhost:9000/faucet")
        print(f"Paste your address: {keypair.address}")
        await asyncio.sleep(5)
        
        balance = await client.get_balance(keypair.address)
        print(f"Balance: {balance} AIC")
        
        if balance < 1000000:
            raise ValueError("Insufficient balance. Please fund from faucet.")
        
        print("Submitting AI inference job...")
        ai_helper = AIJobHelper(client, keypair)
        
        model_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        input_data = json.dumps({
            "prompt": "Explain blockchain in one sentence",
            "temperature": 0.7,
        }).encode()
        
        tx = await ai_helper.submit_job(
            model_hash=model_hash,
            input_data=input_data,
            aic_amount=100000
        )
        
        tx_hash = await client.send_transaction(tx)
        print(f"Job submitted! Transaction: {tx_hash}")
        
        receipt = await client.wait_for_transaction(tx_hash)
        print(f"Transaction confirmed in slot: {receipt.block_slot}")
        
        job_id = tx.hash
        
        print("Waiting for AI provider to complete job...")
        job = await ai_helper.wait_for_job_completion(job_id, timeout=60.0)
        
        print("Job completed!")
        print(f"Status: {job.status}")
        print(f"Provider: {job.provider}")
        
        if job.result:
            result_text = job.result.decode()
            print(f"Result: {result_text}")
        
        if job.vcr:
            print("Verifying compute receipt...")
            verification = await ai_helper.verify_vcr(job.vcr)
            print(f"KZG proof valid: {verification['kzg_valid']}")
            print(f"TEE attestation valid: {verification['tee_valid']}")
        
        print("Success! You just ran verifiable AI compute on Aether.")

if __name__ == "__main__":
    asyncio.run(main())
```

### Step 3: Run (2 minutes)

```bash
python main.py
```

## Understanding the Flow

### 1. Job Submission (Creator Side)

```
Creator → Lock AIC tokens → Submit job → Get job ID
```

- Jobs are held in escrow until completion
- Input data and model hash are recorded on-chain
- Job enters "pending" state

### 2. Job Execution (Provider Side)

```
Provider → Accept job → Run inference → Generate VCR → Submit result
```

- Providers monitor pending jobs
- Execute AI inference in TEE (Trusted Execution Environment)
- Generate cryptographic proof (KZG commitment + TEE attestation)
- Submit result with VCR

### 3. Verification & Settlement

```
On-chain → Verify VCR → Challenge period → Release payment
```

- KZG commitments prove computation integrity
- TEE attestation proves secure execution
- 7-day challenge period (testnet: 1 hour)
- Anyone can challenge invalid results

## Key Concepts

### Verifiable Compute Receipt (VCR)

A VCR contains:
- **Result**: The actual AI inference output
- **KZG Commitments**: Cryptographic proof of computation
- **TEE Attestation**: Hardware-backed proof of secure execution
- **Execution Trace**: Hash of intermediate computation steps

### Economic Security

- Providers stake AIC tokens
- Invalid results can be challenged
- Slashing for malicious behavior
- Rewards for honest computation

### Model Registry

Register your model:

```typescript
const modelHelper = new ModelHelper(client);
await modelHelper.registerModel(modelHash, {
  name: 'GPT-3.5-Turbo',
  version: '1.0.0',
  description: 'Large language model',
  inputSchema: { type: 'object', properties: { prompt: { type: 'string' } } },
  outputSchema: { type: 'object', properties: { output: { type: 'string' } } }
});
```

## Next Steps

### Advanced Examples

1. **Batch Jobs**: Submit multiple jobs in parallel
2. **Model Training**: Run distributed training jobs
3. **Provider Node**: Set up your own compute provider
4. **Result Challenges**: Challenge invalid results for rewards

### Production Checklist

- [ ] Use secure key management (HSM/KMS)
- [ ] Implement error handling and retries
- [ ] Monitor job status and reputation scores
- [ ] Set appropriate AIC pricing
- [ ] Configure challenge response automation
- [ ] Enable metrics and alerting

## Troubleshooting

### "Insufficient balance"
Request tokens from faucet: `http://localhost:9000/faucet`

### "Job timeout"
No providers online. Run a provider node:
```bash
cargo run --bin aether-worker
```

### "Invalid VCR"
Provider may be malicious. Challenge the result:
```typescript
await aiHelper.challengeResult(jobId, 10000n);
```

## Resources

- **API Documentation**: `/docs/api/`
- **SDK Examples**: `/sdk/examples/`
- **Provider Guide**: `/docs/provider-guide.md`
- **Model Registry**: `/docs/model-registry.md`
- **Discord**: https://discord.gg/aether

## Congratulations!

You've successfully submitted and verified your first AI compute job on Aether.

Estimated completion time: 8 minutes

Key achievements:
- Generated a keypair
- Funded your account
- Submitted a job with AIC payment
- Tracked job to completion
- Verified cryptographic proofs

**Now you're ready to build decentralized AI applications on Aether.**

