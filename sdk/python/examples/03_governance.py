"""
Example 3: Governance Participation

Demonstrates:
- Viewing active proposals
- Creating a proposal
- Voting on proposals
- Checking proposal status
"""

import asyncio
from aether import AetherClient, Keypair, GovernanceHelper


async def main():
    async with AetherClient(rpc_url="http://localhost:8545") as client:
        keypair = Keypair.from_seed("my seed phrase")

        gov = GovernanceHelper(client, keypair)

        print("Fetching active proposals...")
        proposals = await gov.get_active_proposals()

        print(f"Found {len(proposals)} active proposals\n")

        for p in proposals:
            print(f"Proposal #{p.id}: {p.title}")
            print(f"- Status: {p.status}")
            print(f"- Votes For: {p.votes_for}")
            print(f"- Votes Against: {p.votes_against}")

            status = await gov.get_proposal_status(p.id)
            if status:
                print(f"- Has Quorum: {status['has_quorum']}")
                print(f"- Time Remaining: {status['time_remaining']} slots")
            print()

        print("Creating a new proposal...")
        create_tx = await gov.create_proposal(
            "Increase Validator Rewards",
            "Proposal to increase validator block rewards from 10 AIC to 15 AIC",
            100800
        )

        create_hash = await client.send_transaction(create_tx)
        print(f"Proposal created: {create_hash}")
        await client.wait_for_transaction(create_hash)

        if proposals:
            proposal_id = proposals[0].id
            print(f"\nVoting on proposal #{proposal_id}...")

            voting_power = await gov.get_voting_power(keypair.address)
            print(f"Your voting power: {voting_power}")

            vote_tx = await gov.vote(proposal_id, True)
            vote_hash = await client.send_transaction(vote_tx)
            print(f"Vote submitted: {vote_hash}")
            await client.wait_for_transaction(vote_hash)
            print("Vote confirmed")

            my_vote = await gov.get_vote(proposal_id, keypair.address)
            if my_vote:
                print("\nYour vote:")
                print(f"- Support: {my_vote.support}")
                print(f"- Power: {my_vote.voting_power}")


if __name__ == "__main__":
    asyncio.run(main())

