"""
Example 5: Batch AI Job Processing

Demonstrates:
- Submitting multiple jobs in parallel
- Tracking multiple jobs
- Aggregating results
"""

import asyncio
import json
from aether import AetherClient, Keypair, AIJobHelper


async def submit_job(ai, client, keypair, model_hash, prompt, index):
    """Submit a single job"""
    input_data = json.dumps({"prompt": prompt}).encode()

    nonce = await client.get_nonce(keypair.address)
    tx = await ai.submit_job(model_hash, input_data, 50000)

    tx_hash = await client.send_transaction(tx)
    print(f"Job {index + 1} submitted: {tx_hash}")

    await client.wait_for_transaction(tx_hash)
    return {"job_id": tx.hash, "prompt": prompt, "index": index}


async def wait_for_job(ai, job_id, prompt, index):
    """Wait for job completion"""
    try:
        job = await ai.wait_for_job_completion(job_id, timeout=180.0)

        result = job.result.decode() if job.result else None

        return {
            "index": index,
            "prompt": prompt,
            "success": True,
            "result": result,
            "provider": job.provider,
        }
    except Exception as error:
        return {
            "index": index,
            "prompt": prompt,
            "success": False,
            "error": str(error),
        }


async def main():
    async with AetherClient(rpc_url="http://localhost:8545") as client:
        keypair = Keypair.from_seed("my seed phrase")

        ai = AIJobHelper(client, keypair)

        model_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"

        prompts = [
            "Summarize blockchain technology",
            "Explain smart contracts",
            "What is proof of stake?",
            "Describe consensus mechanisms",
            "What are verifiable compute receipts?",
        ]

        print(f"Submitting {len(prompts)} jobs in parallel...")

        job_tasks = [
            submit_job(ai, client, keypair, model_hash, prompt, i)
            for i, prompt in enumerate(prompts)
        ]

        jobs = await asyncio.gather(*job_tasks)
        print(f"\nAll {len(jobs)} jobs submitted!")

        print("\nWaiting for completions...")

        completion_tasks = [
            wait_for_job(ai, job["job_id"], job["prompt"], job["index"])
            for job in jobs
        ]

        results = await asyncio.gather(*completion_tasks)

        print("\n=== Results ===\n")

        successful = [r for r in results if r["success"]]
        failed = [r for r in results if not r["success"]]

        for result in successful:
            print(f"Job {result['index'] + 1}: {result['prompt']}")
            print(f"Provider: {result['provider']}")
            print(f"Result: {result['result']}")
            print()

        print(f"\nSummary: {len(successful)}/{len(results)} jobs completed")

        if failed:
            print("\nFailed jobs:")
            for result in failed:
                print(f"- Job {result['index'] + 1}: {result['error']}")

        stats = await ai.get_job_stats()
        print("\nNetwork Statistics:")
        print(f"- Total Jobs: {stats['totalJobs']}")
        print(f"- Pending Jobs: {stats['pendingJobs']}")
        print(f"- Completed Jobs: {stats['completedJobs']}")
        print(f"- Total Volume: {stats['totalVolume']} AIC")


if __name__ == "__main__":
    asyncio.run(main())

