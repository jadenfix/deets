"""
Example 4: AI Job Submission and Tracking

Demonstrates:
- Submitting an AI inference job
- Tracking job status
- Verifying results
- Handling VCR
"""

import asyncio
import json
from aether import AetherClient, Keypair, AIJobHelper


async def main():
    async with AetherClient(rpc_url="http://localhost:8545") as client:
        keypair = Keypair.from_seed("my seed phrase")

        ai = AIJobHelper(client, keypair)

        model_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"

        input_data = json.dumps({
            "prompt": "Write a haiku about blockchain",
            "temperature": 0.7,
            "maxTokens": 50,
        }).encode()

        print("Submitting AI job...")
        submit_tx = await ai.submit_job(model_hash, input_data, 100000)

        tx_hash = await client.send_transaction(submit_tx)
        print(f"Job submitted: {tx_hash}")

        receipt = await client.wait_for_transaction(tx_hash)
        print(f"Transaction confirmed in slot: {receipt.block_slot}")

        job_id = submit_tx.hash
        print(f"Job ID: {job_id}")

        print("\nWaiting for provider to accept and complete job...")

        job = await ai.get_job(job_id)
        print(f"Initial status: {job.status if job else 'not found'}")

        try:
            job = await ai.wait_for_job_completion(job_id, timeout=120.0)
            print("\nJob completed!")
            print(f"- Status: {job.status}")
            print(f"- Provider: {job.provider}")

            if job.result:
                result_text = job.result.decode()
                print(f"- Result: {result_text}")

            if job.vcr:
                print("\nVerifying Compute Receipt...")
                verification = await ai.verify_vcr(job.vcr)
                print(f"- Valid: {verification['valid']}")
                print(f"- KZG Proof Valid: {verification['kzg_valid']}")
                print(f"- TEE Attestation Valid: {verification['tee_valid']}")

                if verification['valid']:
                    print("\nResult is cryptographically verified!")
                else:
                    print("\nWarning: Invalid VCR detected!")
                    print("Consider challenging this result")

            reputation = await ai.get_provider_reputation(job.provider)
            print("\nProvider Reputation:")
            print(f"- Score: {reputation['score']}")
            print(f"- Completed Jobs: {reputation['completedJobs']}")
            print(f"- Average Time: {reputation['averageTime']} seconds")

        except Exception as error:
            print(f"Job failed or timed out: {error}")

            job = await ai.get_job(job_id)
            if job and job.status == "pending":
                print("No providers available. Run an AI provider node.")


if __name__ == "__main__":
    asyncio.run(main())

