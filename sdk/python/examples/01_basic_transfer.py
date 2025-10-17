"""
Example 1: Basic AIC Token Transfer

Demonstrates:
- Keypair generation
- Balance checking
- Simple transfer
- Transaction confirmation
"""

import asyncio
from aether import AetherClient, Keypair, TransactionHelper


async def main():
    async with AetherClient(rpc_url="http://localhost:8545") as client:
        sender = Keypair.from_seed("sender seed phrase")
        recipient = Keypair.generate()

        print(f"Sender: {sender.address}")
        print(f"Recipient: {recipient.address}")

        balance = await client.get_balance(sender.address)
        print(f"Sender balance: {balance} AIC")

        if balance < 1000:
            raise ValueError("Insufficient balance")

        nonce = await client.get_nonce(sender.address)

        tx = await TransactionHelper.create_transfer(
            sender, recipient.address, 1000, nonce
        )

        print("Sending transaction...")
        tx_hash = await client.send_transaction(tx)
        print(f"Transaction hash: {tx_hash}")

        receipt = await client.wait_for_transaction(tx_hash)
        print(f"Confirmed in slot: {receipt.block_slot}")
        print(f"Status: {receipt.status}")

        new_balance = await client.get_balance(recipient.address)
        print(f"Recipient new balance: {new_balance} AIC")


if __name__ == "__main__":
    asyncio.run(main())

