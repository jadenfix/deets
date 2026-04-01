from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, Optional


@dataclass(frozen=True)
class ClientConfig:
    default_fee: int = 2_000_000
    default_gas_limit: int = 500_000


@dataclass
class SubmitResponse:
    tx_hash: str
    accepted: bool


@dataclass
class JobRequest:
    job_id: str
    model_hash: str
    input_hash: str
    max_fee: int
    expires_at: int
    metadata: Optional[Dict[str, object]] = None


@dataclass
class JobSubmission:
    url: str
    method: str
    headers: Dict[str, str]
    body: JobRequest


def ensure_hex(value: str, *, field: str) -> None:
    if not value.startswith("0x"):
        raise ValueError(f"{field} must be a hex string")
    if len(value) == 2:
        raise ValueError(f"{field} must not be empty")
    try:
        int(value[2:], 16)
    except ValueError as exc:
        raise ValueError(f"{field} must be valid hex") from exc


def ensure_positive_int(value: int, *, field: str) -> None:
    if value <= 0:
        raise ValueError(f"{field} must be positive")
