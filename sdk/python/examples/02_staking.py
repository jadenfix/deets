"""
Example 2: Staking and Delegation

Demonstrates:
- Viewing validators
- Delegating stake
- Checking rewards
- Claiming rewards
"""

import asyncio
from aether import AetherClient, Keypair, StakingHelper


async def main():
    async with AetherClient(rpc_url="http://localhost:8545") as client:
        keypair = Keypair.from_seed("my seed phrase")

        staking = StakingHelper(client, keypair)

        print("Fetching validators...")
        validators = await staking.get_validators()

        print(f"Found {len(validators)} active validators")
        for v in validators[:5]:
            print(f"- {v.address}: {v.stake} stake, {v.commission/100}% commission")

        best_validator = max(
            validators,
            key=lambda v: (v.uptime, -v.commission)
        )

        print(f"\nDelegating to best validator: {best_validator.address}")

        delegate_tx = await staking.delegate(best_validator.address, 10000)
        tx_hash = await client.send_transaction(delegate_tx)
        print(f"Delegation tx: {tx_hash}")

        await client.wait_for_transaction(tx_hash)
        print("Delegation confirmed")

        print("\nChecking pending rewards...")
        rewards = await staking.get_pending_rewards(keypair.address)
        print(f"Pending rewards: {rewards} AIC")

        if rewards > 0:
            print("Claiming rewards...")
            claim_tx = await staking.claim_rewards()
            claim_hash = await client.send_transaction(claim_tx)
            await client.wait_for_transaction(claim_hash)
            print("Rewards claimed!")

        delegation = await staking.get_delegation(keypair.address, best_validator.address)
        if delegation:
            print("\nDelegation info:")
            print(f"- Amount: {delegation.amount}")
            print(f"- Rewards: {delegation.rewards}")


if __name__ == "__main__":
    asyncio.run(main())

