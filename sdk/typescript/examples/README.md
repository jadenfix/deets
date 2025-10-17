# Aether SDK Examples

Complete examples demonstrating all major use cases.

## Running Examples

```bash
npm install
npx ts-node examples/01-basic-transfer.ts
```

## Examples

### 1. Basic Transfer (`01-basic-transfer.ts`)
Simple AIC token transfer between accounts.

**Topics**: Keypair, balance checking, transfers, transaction confirmation

### 2. Staking (`02-staking.ts`)
Validator delegation and reward claiming.

**Topics**: Validators, delegation, rewards, staking operations

### 3. Governance (`03-governance.ts`)
Proposal creation and voting.

**Topics**: Proposals, voting, governance participation

### 4. AI Jobs (`04-ai-job.ts`)
Submit and track a verifiable AI compute job.

**Topics**: Job submission, VCR verification, result tracking

### 5. Batch Jobs (`05-batch-jobs.ts`)
Process multiple AI jobs in parallel.

**Topics**: Parallel operations, batch processing, result aggregation

## Prerequisites

- Node.js 18+
- Running Aether node at `http://localhost:8545`
- Funded account (use faucet)

## Next Steps

- Read the [API Documentation](/docs/api/)
- Try the [Hello AIC Job Tutorial](/docs/tutorials/hello-aic-job.md)
- Build your own application

