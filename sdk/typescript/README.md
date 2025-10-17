# Aether TypeScript SDK

Official TypeScript SDK for interacting with the Aether blockchain.

## Installation

```bash
npm install @aether/sdk
```

## Quick Start

```typescript
import { AetherClient, AetherKeypair, AIJobHelper } from '@aether/sdk';

// Connect to Aether node
const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });

// Generate keypair
const keypair = await AetherKeypair.generate();
console.log('Address:', keypair.address);

// Check balance
const balance = await client.getBalance(keypair.address);
console.log('Balance:', balance);

// Submit AI job
const aiHelper = new AIJobHelper(client, keypair);
const tx = await aiHelper.submitJob(
  '0xmodel_hash...',
  new TextEncoder().encode('input data'),
  1000000n // 1 AIC
);
await client.sendTransaction(tx);
```

## Features

- **Full RPC Client**: Complete JSON-RPC interface
- **Transaction Building**: Easy transaction creation and signing
- **Staking**: Validator registration and delegation
- **Governance**: Proposal creation and voting
- **AI Jobs**: Submit and track verifiable compute jobs
- **TypeScript**: Full type safety and IntelliSense

## Documentation

See `/docs/sdk/` for comprehensive documentation.

## Examples

Check `/sdk/typescript/examples/` for more examples.

## License

Apache-2.0

