import time

from aether_sdk import AetherClient


def test_hello_aic_job_submission_flow():
    client = AetherClient("https://rpc.aether.local")
    expiry = int(time.time()) + 3600

    submission = (
        client.job()
        .id("hello-aic-job")
        .model("0x" + "12" * 32)
        .input("0x" + "ab" * 32)
        .max_fee(500_000_000)
        .expires_at(expiry)
        .with_metadata(
            {"prompt": "Generate a haiku about verifiable compute.", "priority": "gold"}
        )
        .to_submission()
    )

    assert submission.url == "https://rpc.aether.local/v1/jobs"
    assert submission.method == "POST"
    assert submission.body.job_id == "hello-aic-job"
    assert submission.body.expires_at == expiry
    assert submission.body.max_fee == 500_000_000
    assert submission.body.metadata is not None
    assert submission.body.metadata["priority"] == "gold"

    prepared = client.prepare_job_submission(submission.body)
    assert prepared == submission
