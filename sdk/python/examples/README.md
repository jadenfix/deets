# Aether Python SDK Examples

Complete examples demonstrating all major use cases.

## Running Examples

```bash
pip install aether-sdk
python examples/01_basic_transfer.py
```

## Examples

### 1. Basic Transfer (`01_basic_transfer.py`)
Simple AIC token transfer between accounts.

**Topics**: Keypair, balance checking, transfers, transaction confirmation

### 2. Staking (`02_staking.py`)
Validator delegation and reward claiming.

**Topics**: Validators, delegation, rewards, staking operations

### 3. Governance (`03_governance.py`)
Proposal creation and voting.

**Topics**: Proposals, voting, governance participation

### 4. AI Jobs (`04_ai_job.py`)
Submit and track a verifiable AI compute job.

**Topics**: Job submission, VCR verification, result tracking

### 5. Batch Jobs (`05_batch_jobs.py`)
Process multiple AI jobs in parallel using asyncio.

**Topics**: Async/await, parallel operations, batch processing

## Prerequisites

- Python 3.8+
- Running Aether node at `http://localhost:8545`
- Funded account (use faucet)

## Next Steps

- Read the [API Documentation](/docs/api/)
- Try the [Hello AIC Job Tutorial](/docs/tutorials/hello-aic-job.md)
- Build your own application

