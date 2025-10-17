"""
AI Job submission and Verifiable Compute Receipt tracking

Core functionality for Aether's AI marketplace.
"""

import asyncio
from typing import Optional, List, Dict
from .client import AetherClient
from .keypair import Keypair
from .transaction import Transaction, TransactionBuilder
from .types import AIJob, VerifiableComputeReceipt, Address, Hash

JOB_ESCROW_CONTRACT = "0x1000000000000000000000000000000000000003"


class AIJobHelper:
    """Helper for AI job operations"""

    def __init__(self, client: AetherClient, keypair: Optional[Keypair] = None):
        self.client = client
        self.keypair = keypair

    async def get_job(self, job_id: Hash) -> Optional[AIJob]:
        """Get job by ID"""
        try:
            data = await self.client._call("ai_getJob", [job_id])
            return AIJob(**data) if data else None
        except:
            return None

    async def get_jobs_by_creator(self, creator: Address) -> List[AIJob]:
        """Get all jobs for a creator"""
        data = await self.client._call("ai_getJobsByCreator", [creator])
        return [AIJob(**j) for j in data]

    async def get_pending_jobs(self) -> List[AIJob]:
        """Get all pending jobs"""
        data = await self.client._call("ai_getPendingJobs", [])
        return [AIJob(**j) for j in data]

    async def get_jobs_by_provider(self, provider: Address) -> List[AIJob]:
        """Get jobs assigned to a provider"""
        data = await self.client._call("ai_getJobsByProvider", [provider])
        return [AIJob(**j) for j in data]

    async def submit_job(
        self, model_hash: Hash, input_data: bytes, aic_amount: int
    ) -> Transaction:
        """Submit a new AI job"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        if aic_amount <= 0:
            raise ValueError("AIC amount must be positive")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_job_submission(model_hash, input_data)

        return await TransactionBuilder.call(
            self.keypair.address, JOB_ESCROW_CONTRACT, data, nonce, aic_amount
        ).sign(self.keypair)

    async def accept_job(self, job_id: Hash) -> Transaction:
        """Accept a job as a provider"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("acceptJob", [job_id])

        return await TransactionBuilder.call(
            self.keypair.address, JOB_ESCROW_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def submit_result(
        self, job_id: Hash, result: bytes, vcr: VerifiableComputeReceipt
    ) -> Transaction:
        """Submit job result with VCR"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_result_submission(job_id, result, vcr)

        return await TransactionBuilder.call(
            self.keypair.address, JOB_ESCROW_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def challenge_result(self, job_id: Hash, challenge_stake: int) -> Transaction:
        """Challenge a job result"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        if challenge_stake <= 0:
            raise ValueError("Challenge stake must be positive")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("challengeResult", [job_id])

        return await TransactionBuilder.call(
            self.keypair.address, JOB_ESCROW_CONTRACT, data, nonce, challenge_stake
        ).sign(self.keypair)

    async def claim_payment(self, job_id: Hash) -> Transaction:
        """Claim job payment"""
        if not self.keypair:
            raise ValueError("Keypair required for signing transactions")

        nonce = await self.client.get_nonce(self.keypair.address)
        data = self._encode_call("claimPayment", [job_id])

        return await TransactionBuilder.call(
            self.keypair.address, JOB_ESCROW_CONTRACT, data, nonce
        ).sign(self.keypair)

    async def get_vcr(self, job_id: Hash) -> Optional[VerifiableComputeReceipt]:
        """Get VCR for a job"""
        try:
            data = await self.client._call("ai_getVCR", [job_id])
            return VerifiableComputeReceipt(**data) if data else None
        except:
            return None

    async def verify_vcr(self, vcr: VerifiableComputeReceipt) -> Dict[str, bool]:
        """Verify a VCR"""
        return await self.client._call("ai_verifyVCR", [vcr.__dict__])

    async def wait_for_job_completion(
        self, job_id: Hash, timeout: float = 300.0, poll_interval: float = 2.0
    ) -> AIJob:
        """Wait for job completion"""
        start_time = asyncio.get_event_loop().time()

        while asyncio.get_event_loop().time() - start_time < timeout:
            job = await self.get_job(job_id)

            if not job:
                raise ValueError(f"Job {job_id} not found")

            if job.status in ["completed", "settled"]:
                return job

            if job.status == "challenged":
                raise ValueError(f"Job {job_id} is being challenged")

            await asyncio.sleep(poll_interval)

        raise TimeoutError(f"Job {job_id} did not complete within {timeout}s")

    async def get_job_stats(self) -> Dict:
        """Get job statistics"""
        return await self.client._call("ai_getJobStats", [])

    async def get_provider_reputation(self, provider: Address) -> Dict:
        """Get provider reputation"""
        return await self.client._call("ai_getProviderReputation", [provider])

    def _encode_job_submission(self, model_hash: Hash, input_data: bytes) -> bytes:
        """Encode job submission call data"""
        method = b"submitJob"[:4]
        hash_bytes = bytes.fromhex(model_hash[2:])
        return method + hash_bytes + input_data

    def _encode_result_submission(
        self, job_id: Hash, result: bytes, vcr: VerifiableComputeReceipt
    ) -> bytes:
        """Encode result submission call data"""
        method = b"submitResult"[:4]
        job_id_bytes = bytes.fromhex(job_id[2:])
        return method + job_id_bytes + result

    def _encode_call(self, method: str, params: List) -> bytes:
        """Simple function selector encoding"""
        import json
        signature = method + json.dumps(params)
        return signature.encode()[:4]


class ModelHelper:
    """Helper for model registry operations"""

    def __init__(self, client: AetherClient):
        self.client = client

    async def register_model(self, model_hash: Hash, metadata: Dict) -> None:
        """Register a model"""
        await self.client._call("ai_registerModel", [model_hash, metadata])

    async def get_model(self, model_hash: Hash) -> Optional[Dict]:
        """Get model metadata"""
        try:
            return await self.client._call("ai_getModel", [model_hash])
        except:
            return None

    async def list_models(self) -> List[Hash]:
        """List all registered models"""
        return await self.client._call("ai_listModels", [])

