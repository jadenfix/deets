# Aether API Documentation

Comprehensive reference for Aether's JSON-RPC API and SDK methods.

## Table of Contents

1. [JSON-RPC API](#json-rpc-api)
2. [TypeScript SDK](#typescript-sdk)
3. [Python SDK](#python-sdk)
4. [Error Codes](#error-codes)
5. [Rate Limits](#rate-limits)

## JSON-RPC API

### Endpoint

```
HTTP POST http://localhost:8545/
Content-Type: application/json
```

### Request Format

```json
{
  "jsonrpc": "2.0",
  "method": "methodName",
  "params": [param1, param2],
  "id": 1
}
```

### Response Format

Success:
```json
{
  "jsonrpc": "2.0",
  "result": {...},
  "id": 1
}
```

Error:
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32600,
    "message": "Invalid request"
  },
  "id": 1
}
```

## Core Methods

### getSlot

Get current blockchain slot number.

**Parameters**: None

**Returns**: `number` - Current slot

**Example**:
```json
{"jsonrpc": "2.0", "method": "getSlot", "params": [], "id": 1}
```

Response:
```json
{"jsonrpc": "2.0", "result": 12345, "id": 1}
```

### getBlock

Get block information by slot number.

**Parameters**:
- `slot` (number): Block slot
- `includeTransactions` (boolean, optional): Include full transactions (default: false)

**Returns**: `Block` - Block information

**Example**:
```json
{"jsonrpc": "2.0", "method": "getBlock", "params": [12345, true], "id": 1}
```

### getTransaction

Get transaction by hash.

**Parameters**:
- `hash` (string): Transaction hash (0x-prefixed hex)

**Returns**: `Transaction | null`

**Example**:
```json
{"jsonrpc": "2.0", "method": "getTransaction", "params": ["0xabc123..."], "id": 1}
```

### getAccount

Get account information.

**Parameters**:
- `address` (string): Account address (0x-prefixed)

**Returns**: `Account`

**Example**:
```json
{"jsonrpc": "2.0", "method": "getAccount", "params": ["0x742d35Cc..."], "id": 1}
```

Response:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "address": "0x742d35Cc...",
    "balance": "1000000",
    "nonce": 5,
    "codeHash": null
  },
  "id": 1
}
```

### sendTransaction

Send a signed transaction.

**Parameters**:
- `transaction` (object): Signed transaction object

**Returns**: `string` - Transaction hash

**Example**:
```json
{
  "jsonrpc": "2.0",
  "method": "sendTransaction",
  "params": [{
    "from": "0x742d35Cc...",
    "to": "0x8f3CF7ad...",
    "value": "1000",
    "nonce": 5,
    "signature": "0xabc123..."
  }],
  "id": 1
}
```

### getTransactionReceipt

Get transaction receipt (confirmation).

**Parameters**:
- `hash` (string): Transaction hash

**Returns**: `TransactionReceipt | null`

**Example**:
```json
{"jsonrpc": "2.0", "method": "getTransactionReceipt", "params": ["0xabc123..."], "id": 1}
```

## Staking Methods

### staking_getValidator

Get validator information.

**Parameters**:
- `address` (string): Validator address

**Returns**: `Validator | null`

### staking_getValidators

Get all active validators.

**Parameters**: None

**Returns**: `Validator[]`

### staking_getDelegation

Get delegation information.

**Parameters**:
- `delegator` (string): Delegator address
- `validator` (string): Validator address

**Returns**: `Delegation | null`

### staking_getPendingRewards

Get pending staking rewards.

**Parameters**:
- `address` (string): Account address

**Returns**: `string` - Reward amount

### staking_getTotalStake

Get total staked amount in the network.

**Parameters**: None

**Returns**: `string` - Total stake

## Governance Methods

### governance_getProposal

Get proposal by ID.

**Parameters**:
- `proposalId` (number): Proposal ID

**Returns**: `Proposal | null`

### governance_getActiveProposals

Get all active proposals.

**Parameters**: None

**Returns**: `Proposal[]`

### governance_getVote

Get vote information.

**Parameters**:
- `proposalId` (number): Proposal ID
- `voter` (string): Voter address

**Returns**: `Vote | null`

### governance_getVotingPower

Get voting power for an address.

**Parameters**:
- `address` (string): Account address

**Returns**: `string` - Voting power

## AI Methods

### ai_getJob

Get AI job by ID.

**Parameters**:
- `jobId` (string): Job hash

**Returns**: `AIJob | null`

**Example**:
```json
{"jsonrpc": "2.0", "method": "ai_getJob", "params": ["0xjob123..."], "id": 1}
```

Response:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "id": "0xjob123...",
    "creator": "0x742d35Cc...",
    "modelHash": "0xmodel123...",
    "inputData": "0x...",
    "aicLocked": "100000",
    "status": "completed",
    "provider": "0x8f3CF7ad...",
    "result": "0x...",
    "vcr": {...}
  },
  "id": 1
}
```

### ai_getJobsByCreator

Get all jobs for a creator.

**Parameters**:
- `creator` (string): Creator address

**Returns**: `AIJob[]`

### ai_getPendingJobs

Get all pending jobs (available for providers).

**Parameters**: None

**Returns**: `AIJob[]`

### ai_getVCR

Get Verifiable Compute Receipt for a job.

**Parameters**:
- `jobId` (string): Job hash

**Returns**: `VerifiableComputeReceipt | null`

### ai_verifyVCR

Verify a VCR's cryptographic proofs.

**Parameters**:
- `vcr` (object): Verifiable Compute Receipt

**Returns**: `object` - Verification result
```json
{
  "valid": true,
  "kzgValid": true,
  "teeValid": true
}
```

### ai_getProviderReputation

Get reputation score for a provider.

**Parameters**:
- `provider` (string): Provider address

**Returns**: `object` - Reputation data
```json
{
  "score": 0.95,
  "completedJobs": 1000,
  "failedJobs": 5,
  "averageTime": 120.5
}
```

### ai_registerModel

Register a model in the registry.

**Parameters**:
- `modelHash` (string): Model hash
- `metadata` (object): Model metadata

**Returns**: `boolean` - Success

### ai_getModel

Get model metadata.

**Parameters**:
- `modelHash` (string): Model hash

**Returns**: `object | null` - Model information

## TypeScript SDK

### Installation

```bash
npm install @aether/sdk
```

### Client

#### AetherClient

```typescript
import { AetherClient } from '@aether/sdk';

const client = new AetherClient({
  rpcUrl: 'http://localhost:8545',
  chainId: 1,
  timeout: 30000
});

await client.getSlot();
await client.getBalance('0x...');
await client.sendTransaction(signedTx);
```

**Methods**:
- `getSlot(): Promise<number>`
- `getBlock(slot, includeTransactions?): Promise<Block>`
- `getTransaction(hash): Promise<SignedTransaction | null>`
- `getAccount(address): Promise<Account>`
- `getBalance(address): Promise<bigint>`
- `getNonce(address): Promise<number>`
- `sendTransaction(tx): Promise<Hash>`
- `getTransactionReceipt(hash): Promise<TransactionReceipt | null>`
- `waitForTransaction(hash, timeout?, pollInterval?): Promise<TransactionReceipt>`
- `isHealthy(): Promise<boolean>`

### Keypair

#### AetherKeypair

```typescript
import { AetherKeypair } from '@aether/sdk';

const keypair = await AetherKeypair.generate();
const keypair2 = await AetherKeypair.fromSeed('my seed phrase');
const keypair3 = await AetherKeypair.fromSecretKeyHex('0xabc123...');

const signature = await keypair.sign(message);
const valid = await AetherKeypair.verify(signature, message, publicKey);
```

**Methods**:
- `static generate(): Promise<AetherKeypair>`
- `static fromSeed(seed): Promise<AetherKeypair>`
- `static fromSecretKey(key): Promise<AetherKeypair>`
- `static fromSecretKeyHex(hex): Promise<AetherKeypair>`
- `sign(message): Promise<Signature>`
- `static verify(sig, msg, pubkey): Promise<boolean>`
- `toSecretKeyHex(): string`
- `toPublicKeyHex(): string`

**Properties**:
- `publicKey: Uint8Array`
- `secretKey: Uint8Array`
- `address: Address`

### Transaction Building

#### TransactionBuilder

```typescript
import { TransactionBuilder } from '@aether/sdk';

const tx = await TransactionBuilder
  .transfer(from, to, amount, nonce)
  .sign(keypair);

const callTx = await TransactionBuilder
  .call(from, contract, data, nonce, value)
  .sign(keypair);
```

**Methods**:
- `from(address): this`
- `to(address): this`
- `value(amount): this`
- `data(bytes): this`
- `nonce(n): this`
- `build(): Transaction`
- `sign(keypair): Promise<SignedTransaction>`
- `static transfer(from, to, amount, nonce): TransactionBuilder`
- `static call(from, contract, data, nonce, value?): TransactionBuilder`

### Staking

#### StakingHelper

```typescript
import { StakingHelper } from '@aether/sdk';

const staking = new StakingHelper(client, keypair);

const validators = await staking.getValidators();
const tx = await staking.delegate('0xvalidator...', 1000000n);
await client.sendTransaction(tx);
```

**Methods**:
- `getValidator(address): Promise<Validator | null>`
- `getValidators(): Promise<Validator[]>`
- `getDelegation(delegator, validator): Promise<Delegation | null>`
- `registerValidator(stake, commission): Promise<SignedTransaction>`
- `delegate(validator, amount): Promise<SignedTransaction>`
- `undelegate(validator, amount): Promise<SignedTransaction>`
- `claimRewards(): Promise<SignedTransaction>`
- `getPendingRewards(address): Promise<bigint>`
- `getTotalStake(): Promise<bigint>`

### Governance

#### GovernanceHelper

```typescript
import { GovernanceHelper } from '@aether/sdk';

const gov = new GovernanceHelper(client, keypair);

const proposals = await gov.getActiveProposals();
const tx = await gov.vote(1, true);
await client.sendTransaction(tx);
```

**Methods**:
- `getProposal(id): Promise<Proposal | null>`
- `getActiveProposals(): Promise<Proposal[]>`
- `createProposal(title, description, duration?): Promise<SignedTransaction>`
- `vote(proposalId, support): Promise<SignedTransaction>`
- `executeProposal(proposalId): Promise<SignedTransaction>`
- `getVotingPower(address): Promise<bigint>`
- `hasQuorum(proposalId): Promise<boolean>`

### AI Jobs

#### AIJobHelper

```typescript
import { AIJobHelper } from '@aether/sdk';

const ai = new AIJobHelper(client, keypair);

const tx = await ai.submitJob(modelHash, inputData, aicAmount);
const txHash = await client.sendTransaction(tx);

const job = await ai.waitForJobCompletion(jobId);
console.log('Result:', job.result);
```

**Methods**:
- `getJob(jobId): Promise<AIJob | null>`
- `submitJob(modelHash, inputData, aicAmount): Promise<SignedTransaction>`
- `acceptJob(jobId): Promise<SignedTransaction>`
- `submitResult(jobId, result, vcr): Promise<SignedTransaction>`
- `challengeResult(jobId, stake): Promise<SignedTransaction>`
- `claimPayment(jobId): Promise<SignedTransaction>`
- `getVCR(jobId): Promise<VerifiableComputeReceipt | null>`
- `verifyVCR(vcr): Promise<{valid, kzgValid, teeValid}>`
- `waitForJobCompletion(jobId, timeout?, pollInterval?): Promise<AIJob>`
- `getProviderReputation(provider): Promise<ReputationData>`

## Python SDK

### Installation

```bash
pip install aether-sdk
```

### Client

#### AetherClient

```python
from aether import AetherClient

async with AetherClient(rpc_url="http://localhost:8545") as client:
    slot = await client.get_slot()
    balance = await client.get_balance("0x...")
    await client.send_transaction(tx)
```

**Methods**:
- `async get_slot() -> int`
- `async get_block(slot, include_transactions=False) -> Block`
- `async get_transaction(hash) -> Transaction | None`
- `async get_account(address) -> Account`
- `async get_balance(address) -> int`
- `async get_nonce(address) -> int`
- `async send_transaction(tx) -> Hash`
- `async get_transaction_receipt(hash) -> TransactionReceipt | None`
- `async wait_for_transaction(hash, timeout=30, poll_interval=1) -> TransactionReceipt`
- `async is_healthy() -> bool`

### Keypair

#### Keypair

```python
from aether import Keypair

keypair = Keypair.generate()
keypair2 = Keypair.from_seed("my seed phrase")
keypair3 = Keypair.from_secret_key_hex("0xabc123...")

signature = keypair.sign(message)
valid = Keypair.verify(signature, message, public_key)
```

**Methods**:
- `@classmethod generate() -> Keypair`
- `@classmethod from_seed(seed) -> Keypair`
- `@classmethod from_secret_key(key) -> Keypair`
- `@classmethod from_secret_key_hex(hex) -> Keypair`
- `sign(message) -> Signature`
- `@staticmethod verify(sig, msg, pubkey) -> bool`
- `to_secret_key_hex() -> str`

**Properties**:
- `public_key: bytes`
- `secret_key: bytes`
- `address: Address`

### Transaction Building

#### TransactionBuilder

```python
from aether import TransactionBuilder

tx = await TransactionBuilder.transfer(
    from_addr, to, amount, nonce
).sign(keypair)

call_tx = await TransactionBuilder.call(
    from_addr, contract, data, nonce, value
).sign(keypair)
```

**Methods**:
- `from_addr(address) -> Self`
- `to(address) -> Self`
- `value(amount) -> Self`
- `data(bytes) -> Self`
- `nonce(n) -> Self`
- `build() -> Transaction`
- `async sign(keypair) -> Transaction`
- `@classmethod transfer(from, to, amount, nonce) -> TransactionBuilder`
- `@classmethod call(from, contract, data, nonce, value=0) -> TransactionBuilder`

### Staking

#### StakingHelper

```python
from aether import StakingHelper

staking = StakingHelper(client, keypair)

validators = await staking.get_validators()
tx = await staking.delegate("0xvalidator...", 1000000)
await client.send_transaction(tx)
```

**Methods**:
- `async get_validator(address) -> Validator | None`
- `async get_validators() -> List[Validator]`
- `async register_validator(stake, commission) -> Transaction`
- `async delegate(validator, amount) -> Transaction`
- `async undelegate(validator, amount) -> Transaction`
- `async claim_rewards() -> Transaction`
- `async get_pending_rewards(address) -> int`

### Governance

#### GovernanceHelper

```python
from aether import GovernanceHelper

gov = GovernanceHelper(client, keypair)

proposals = await gov.get_active_proposals()
tx = await gov.vote(1, True)
await client.send_transaction(tx)
```

**Methods**:
- `async get_proposal(id) -> Proposal | None`
- `async get_active_proposals() -> List[Proposal]`
- `async create_proposal(title, description, duration=100800) -> Transaction`
- `async vote(proposal_id, support) -> Transaction`
- `async execute_proposal(proposal_id) -> Transaction`
- `async get_voting_power(address) -> int`

### AI Jobs

#### AIJobHelper

```python
from aether import AIJobHelper

ai = AIJobHelper(client, keypair)

tx = await ai.submit_job(model_hash, input_data, aic_amount)
tx_hash = await client.send_transaction(tx)

job = await ai.wait_for_job_completion(job_id)
print(f"Result: {job.result}")
```

**Methods**:
- `async get_job(job_id) -> AIJob | None`
- `async submit_job(model_hash, input_data, aic_amount) -> Transaction`
- `async accept_job(job_id) -> Transaction`
- `async submit_result(job_id, result, vcr) -> Transaction`
- `async challenge_result(job_id, stake) -> Transaction`
- `async claim_payment(job_id) -> Transaction`
- `async get_vcr(job_id) -> VerifiableComputeReceipt | None`
- `async verify_vcr(vcr) -> Dict[str, bool]`
- `async wait_for_job_completion(job_id, timeout=300, poll_interval=2) -> AIJob`
- `async get_provider_reputation(provider) -> Dict`

## Error Codes

Standard JSON-RPC errors:

| Code | Message | Description |
|------|---------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid request | Invalid request object |
| -32601 | Method not found | Method does not exist |
| -32602 | Invalid params | Invalid method parameters |
| -32603 | Internal error | Internal JSON-RPC error |

Aether-specific errors:

| Code | Message | Description |
|------|---------|-------------|
| -32000 | Server error | Generic server error |
| -32001 | Insufficient balance | Account balance too low |
| -32002 | Invalid signature | Transaction signature invalid |
| -32003 | Nonce too low | Transaction nonce already used |
| -32004 | Gas limit exceeded | Transaction gas limit exceeded |
| -32005 | Not found | Resource not found |
| -32006 | Invalid state | Operation not valid in current state |
| -32007 | Unauthorized | Permission denied |

## Rate Limits

Default rate limits:

- **Public RPC**: 100 requests/minute per IP
- **Authenticated RPC**: 1000 requests/minute per API key
- **WebSocket**: 500 messages/minute per connection

Rate limit headers:
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 95
X-RateLimit-Reset: 1640000000
```

## Best Practices

### Error Handling

```typescript
try {
  const tx = await client.sendTransaction(signedTx);
} catch (error) {
  if (error.message.includes('Insufficient balance')) {
    console.log('Please fund your account');
  } else if (error.message.includes('Invalid signature')) {
    console.log('Transaction signature is invalid');
  } else {
    throw error;
  }
}
```

### Connection Management

```typescript
const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });

// Check health before operations
if (!(await client.isHealthy())) {
  throw new Error('Node is not healthy');
}

// Reuse client instance
for (const tx of transactions) {
  await client.sendTransaction(tx);
}
```

### Polling Patterns

```typescript
const receipt = await client.waitForTransaction(txHash, 30000, 1000);
```

For long-running operations, use exponential backoff:
```typescript
let delay = 1000;
while (true) {
  const job = await ai.getJob(jobId);
  if (job.status === 'completed') break;
  await new Promise(r => setTimeout(r, delay));
  delay = Math.min(delay * 1.5, 10000);
}
```

## Support

- **Documentation**: https://docs.aether.network
- **Discord**: https://discord.gg/aether
- **GitHub**: https://github.com/aether/sdk
- **Email**: support@aether.network

