from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field
from typing import List, Optional

from .types import ensure_hex, ensure_positive_int


@dataclass
class Transaction:
    nonce: int
    sender: str
    sender_public_key: str
    recipient: str
    amount: int
    fee: int
    gas_limit: int
    memo: Optional[str]
    signature: str
    reads: List[str] = field(default_factory=list)
    writes: List[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        ensure_hex(self.sender, field="sender")
        ensure_hex(self.sender_public_key, field="sender_public_key")
        ensure_hex(self.recipient, field="recipient")
        ensure_hex(self.signature, field="signature")
        ensure_positive_int(self.amount, field="amount")
        ensure_positive_int(self.fee, field="fee")
        ensure_positive_int(self.gas_limit, field="gas_limit")
        ensure_positive_int(self.nonce + 1, field="nonce + 1")  # nonce can be 0
        if len(self.signature) < 66:
            raise ValueError("signature must be at least 64 bytes (hex)")
        if not self.writes:
            self.writes.append(self.recipient)

    def hash(self) -> str:
        payload = {
            "nonce": self.nonce,
            "sender": self.sender,
            "sender_public_key": self.sender_public_key,
            "recipient": self.recipient,
            "amount": str(self.amount),
            "fee": str(self.fee),
            "gas_limit": self.gas_limit,
            "memo": self.memo,
            "reads": self.reads,
            "writes": self.writes,
        }
        digest = hashlib.sha256(json.dumps(payload, sort_keys=True).encode())
        return "0x" + digest.hexdigest()
