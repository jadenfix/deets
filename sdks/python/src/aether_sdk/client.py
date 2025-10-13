from __future__ import annotations

from dataclasses import dataclass

from .builders import JobBuilder, TransferBuilder
from .transaction import Transaction
from .types import ClientConfig, JobRequest, JobSubmission, SubmitResponse


def _normalize_endpoint(endpoint: str) -> str:
    if not endpoint:
        raise ValueError("endpoint must be provided")
    return endpoint.rstrip("/")


@dataclass
class AetherClient:
    endpoint: str
    config: ClientConfig = ClientConfig()

    def __post_init__(self) -> None:
        self.endpoint = _normalize_endpoint(self.endpoint)

    def transfer(self) -> TransferBuilder:
        return TransferBuilder(self.config)

    def job(self) -> JobBuilder:
        return JobBuilder(self.endpoint)

    def submit(self, transaction: Transaction) -> SubmitResponse:
        return SubmitResponse(tx_hash=transaction.hash(), accepted=True)

    def prepare_job_submission(self, job: JobRequest) -> JobSubmission:
        return JobSubmission(
            url=f"{self.endpoint}/v1/jobs",
            method="POST",
            headers={"content-type": "application/json"},
            body=job,
        )
