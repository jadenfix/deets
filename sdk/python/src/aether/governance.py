"""
Governance helpers for Aether

Simplifies proposal creation, voting, and execution.
"""

from typing import Optional, List, Dict
from .client import AetherClient
from .keypair import Keypair
from .transaction import Transaction, TransactionBuilder
from .types import Proposal, Vote, Address

GOVERNANCE_CONTRACT = "0x1000000000000000000000000000000000000002"


class GovernanceHelper:
    """Helper for governance operations"""

    def __init__(self, client: AetherClient, keypair: Optional[Keypair] = None):
        self.client = client
        self.keypair = keypair

    async def get_proposal(self, proposal_id: int) -> Optional[Proposal]:
        """Get proposal by ID"""
        try:
            data = await self.client._call("governance_getProposal", [proposal_id])
            return Proposal(**data) if data else None
        except:
            return None

    async def get_active_proposals(self) -> List[Proposal]:
        """Get all active proposals"""
        data = await self.client._call("governance_getActiveProposals", [])
        return [Proposal(**p) for p in data]

    async def get_all_proposals(self) -> List[Proposal]:
        """Get all proposals"""
        data = await self.client._call("governance_getAllProposals", [])
        return [Proposal(**p) for p in data]

    async def get_vote(self, proposal_id: int, voter: Address) -> Optional[Vote]:
        """Get vote for a proposal"""
        try:
            data = await self.client._call("governance_getVote", [proposal_id, voter])
            return Vote(**data) if data else None
        except:
            return None

    async def create_proposal(
        self, title: str, description: str, duration: int = 100800
    ) -> Transaction:
        """Create a new proposal"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        if not (1 <= len(title) <= 256):
            raise ValueError("Title must be between 1 and 256 characters")

        if not (1 <= len(description) <= 10000):
            raise ValueError("Description must be between 1 and 10000 characters")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("createProposal", [title, description, duration])

        return await TransactionBuilder.call(
            self.keypair.address, GOVERNANCE_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def vote(self, proposal_id: int, support: bool) -> Transaction:
        """Vote on a proposal"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("vote", [proposal_id, support])

        return await TransactionBuilder.call(
            self.keypair.address, GOVERNANCE_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def execute_proposal(self, proposal_id: int) -> Transaction:
        """Execute a passed proposal"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        proposal = await self.get_proposal(proposal_id)
        if not proposal:
            raise ValueError("Proposal not found")

        if proposal.status != "passed":
            raise ValueError("Proposal must be in passed state")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("executeProposal", [proposal_id])

        return await TransactionBuilder.call(
            self.keypair.address, GOVERNANCE_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def get_voting_power(self, address: Address) -> int:
        """Get voting power for an address"""
        return await self.client._call("governance_getVotingPower", [address])

    async def get_quorum(self) -> int:
        """Get quorum threshold"""
        return await self.client._call("governance_getQuorum", [])

    async def has_quorum(self, proposal_id: int) -> bool:
        """Check if a proposal has reached quorum"""
        proposal = await self.get_proposal(proposal_id)
        if not proposal:
            return False

        quorum = await self.get_quorum()
        total_votes = proposal.votes_for + proposal.votes_against

        return total_votes >= quorum

    async def get_proposal_status(self, proposal_id: int) -> Optional[Dict]:
        """Get proposal status with context"""
        proposal = await self.get_proposal(proposal_id)
        if not proposal:
            return None

        current_slot = await self.client.get_slot()
        has_quorum = await self.has_quorum(proposal_id)
        time_remaining = max(0, proposal.end_slot - current_slot)
        can_execute = proposal.status == "passed"

        return {
            "proposal": proposal,
            "has_quorum": has_quorum,
            "time_remaining": time_remaining,
            "can_execute": can_execute,
        }

    def _encode_call(self, method: str, params: List) -> bytes:
        """Simple function selector encoding"""
        import json
        signature = method + json.dumps(params)
        return signature.encode()[:4]

