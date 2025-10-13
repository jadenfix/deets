# Aether Python SDK

Lightweight client utilities for submitting transactions and AI jobs to the Aether blockchain.

## Quick Start

```python
from aether_sdk import AetherClient

client = AetherClient("https://rpc.aether.local")
tx = (
    client.transfer()
    .to("0x8b0b54d2248a3a5617b6bd8a2fd4cc8ebc0f2e90")
    .amount(1_000_000)
    .memo("phase7")
    .build(
        sender="0x1111111111111111111111111111111111111111",
        sender_public_key="0x" + "a1" * 32,
        signature="0x" + "b2" * 32,
        nonce=7,
    )
)

response = client.submit(tx)
print(response.tx_hash)
```
