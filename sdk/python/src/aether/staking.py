"""
Staking helpers for Aether

Simplifies validator registration, delegation, and reward claiming.
"""

from typing import Optional, List
from .client import AetherClient
from .keypair import Keypair
from .transaction import Transaction, TransactionBuilder
from .types import Validator, Delegation, Address

STAKING_CONTRACT = "0x1000000000000000000000000000000000000001"


class StakingHelper:
    """Helper for staking operations"""

    def __init__(self, client: AetherClient, keypair: Optional[Keypair] = None):
        self.client = client
        self.keypair = keypair

    async def get_validator(self, address: Address) -> Optional[Validator]:
        """Get validator information"""
        try:
            data = await self.client._call("staking_getValidator", [address])
            return Validator(**data) if data else None
        except:
            return None

    async def get_validators(self) -> List[Validator]:
        """Get all active validators"""
        data = await self.client._call("staking_getValidators", [])
        return [Validator(**v) for v in data]

    async def get_delegation(
        self, delegator: Address, validator: Address
    ) -> Optional[Delegation]:
        """Get delegation information"""
        try:
            data = await self.client._call("staking_getDelegation", [delegator, validator])
            return Delegation(**data) if data else None
        except:
            return None

    async def get_delegations(self, delegator: Address) -> List[Delegation]:
        """Get all delegations for a delegator"""
        data = await self.client._call("staking_getDelegations", [delegator])
        return [Delegation(**d) for d in data]

    async def register_validator(self, stake: int, commission: int) -> Transaction:
        """Register as a validator"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        if commission < 0 or commission > 10000:
            raise ValueError("Commission must be between 0 and 10000 basis points")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("registerValidator", [commission])

        return await TransactionBuilder.call(
            self.keypair.address, STAKING_CONTRACT, data, nonce, stake
        ).sign(self.keypair)

    async def delegate(self, validator: Address, amount: int) -> Transaction:
        """Delegate stake to a validator"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("delegate", [validator])

        return await TransactionBuilder.call(
            self.keypair.address, STAKING_CONTRACT, data, nonce, amount
        ).sign(self.keypair)

    async def undelegate(self, validator: Address, amount: int) -> Transaction:
        """Undelegate stake from a validator"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("undelegate", [validator, amount])

        return await TransactionBuilder.call(
            self.keypair.address, STAKING_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def claim_rewards(self) -> Transaction:
        """Claim staking rewards"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("claimRewards", [])

        return await TransactionBuilder.call(
            self.keypair.address, STAKING_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def get_pending_rewards(self, address: Address) -> int:
        """Get pending rewards"""
        return await self.client._call("staking_getPendingRewards", [address])

    async def get_total_stake(self) -> int:
        """Get total staked amount in the network"""
        return await self.client._call("staking_getTotalStake", [])

    async def get_minimum_stake(self) -> int:
        """Get minimum stake requirement"""
        return await self.client._call("staking_getMinimumStake", [])

    def _encode_call(self, method: str, params: List) -> bytes:
        """Simple function selector encoding"""
        import json
        signature = method + json.dumps(params)
        return signature.encode()[:4]

