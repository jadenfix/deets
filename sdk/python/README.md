# Aether Python SDK

Official Python SDK for interacting with the Aether blockchain with full async/await support.

## Installation

```bash
pip install aether-sdk
```

## Quick Start

```python
import asyncio
from aether import AetherClient, Keypair, AIJobHelper

async def main():
    async with AetherClient(rpc_url="http://localhost:8545") as client:
        keypair = Keypair.generate()
        print(f"Address: {keypair.address}")
        
        balance = await client.get_balance(keypair.address)
        print(f"Balance: {balance}")
        
        ai_helper = AIJobHelper(client, keypair)
        tx = await ai_helper.submit_job(
            model_hash="0xmodel_hash...",
            input_data=b"input data",
            aic_amount=1000000
        )
        
        tx_hash = await client.send_transaction(tx)
        print(f"Transaction sent: {tx_hash}")
        
        receipt = await client.wait_for_transaction(tx_hash)
        print(f"Status: {receipt.status}")

asyncio.run(main())
```

## Features

- **Async/Await**: Full async support for high-performance applications
- **Type Hints**: Complete type annotations for better IDE support
- **RPC Client**: Complete JSON-RPC interface
- **Transaction Building**: Easy transaction creation and signing
- **Staking**: Validator registration and delegation
- **Governance**: Proposal creation and voting
- **AI Jobs**: Submit and track verifiable compute jobs

## Documentation

See `/docs/sdk/` for comprehensive documentation.

## Examples

Check `/sdk/python/examples/` for more examples.

## License

Apache-2.0

