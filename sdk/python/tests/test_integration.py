"""
Integration Tests for Aether Python SDK

Tests end-to-end developer workflows
"""

import pytest
import asyncio
from aether import (
    AetherClient,
    Keypair,
    TransactionHelper,
    StakingHelper,
    GovernanceHelper,
    AIJobHelper,
)


@pytest.fixture
async def client():
    """Create test client"""
    async with AetherClient(rpc_url="http://localhost:8545") as c:
        yield c


@pytest.fixture
def keypair():
    """Create test keypair"""
    return Keypair.from_seed("test-seed-phrase")


@pytest.mark.asyncio
class TestCoreClient:
    async def test_connect_to_node(self, client):
        healthy = await client.is_healthy()
        assert healthy is True

    async def test_get_current_slot(self, client):
        slot = await client.get_slot()
        assert isinstance(slot, int)
        assert slot > 0

    async def test_get_account_balance(self, client, keypair):
        balance = await client.get_balance(keypair.address)
        assert isinstance(balance, int)
        assert balance >= 0


@pytest.mark.asyncio
class TestKeypairManagement:
    async def test_generate_new_keypair(self):
        new_keypair = Keypair.generate()
        assert new_keypair.address.startswith("0x")
        assert len(new_keypair.address) == 42
        assert isinstance(new_keypair.public_key, bytes)
        assert isinstance(new_keypair.secret_key, bytes)

    async def test_create_keypair_from_seed(self):
        keypair1 = Keypair.from_seed("test")
        keypair2 = Keypair.from_seed("test")
        assert keypair1.address == keypair2.address

    async def test_sign_and_verify_message(self, keypair):
        message = b"Hello Aether"
        signature = keypair.sign(message)
        
        valid = Keypair.verify(signature, message, keypair.public_key)
        assert valid is True


@pytest.mark.asyncio
class TestTransactionBuilding:
    async def test_build_unsigned_transaction(self, client, keypair):
        nonce = await client.get_nonce(keypair.address)
        
        tx = await TransactionHelper.create_transfer(
            keypair,
            "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
            1000,
            nonce
        )

        assert tx.from_addr == keypair.address
        assert tx.to == "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb"
        assert tx.value == 1000
        assert tx.signature is not None
        assert tx.hash is not None


@pytest.mark.asyncio
class TestStakingOperations:
    async def test_fetch_validators(self, client):
        staking = StakingHelper(client)
        validators = await staking.get_validators()
        
        assert isinstance(validators, list)
        if validators:
            v = validators[0]
            assert v.address.startswith("0x")
            assert isinstance(v.stake, int)
            assert isinstance(v.commission, int)

    async def test_get_total_stake(self, client):
        staking = StakingHelper(client)
        total_stake = await staking.get_total_stake()
        assert isinstance(total_stake, int)
        assert total_stake >= 0


@pytest.mark.asyncio
class TestGovernanceOperations:
    async def test_fetch_active_proposals(self, client):
        gov = GovernanceHelper(client)
        proposals = await gov.get_active_proposals()
        
        assert isinstance(proposals, list)

    async def test_get_voting_power(self, client, keypair):
        gov = GovernanceHelper(client)
        power = await gov.get_voting_power(keypair.address)
        assert isinstance(power, int)
        assert power >= 0


@pytest.mark.asyncio
class TestAIJobOperations:
    async def test_get_pending_jobs(self, client):
        ai = AIJobHelper(client)
        jobs = await ai.get_pending_jobs()
        
        assert isinstance(jobs, list)

    async def test_get_job_stats(self, client):
        ai = AIJobHelper(client)
        stats = await ai.get_job_stats()
        
        assert "totalJobs" in stats
        assert "completedJobs" in stats
        assert "totalVolume" in stats


@pytest.mark.asyncio
class TestErrorHandling:
    async def test_handle_invalid_address(self, client):
        with pytest.raises(Exception):
            await client.get_balance("invalid")

    async def test_handle_nonexistent_transaction(self, client):
        tx = await client.get_transaction(
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        )
        assert tx is None


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

