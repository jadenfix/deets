"""
Core types for Aether SDK
"""

from typing import Optional, List, Literal
from dataclasses import dataclass

Address = str
Hash = str
Signature = str


@dataclass
class Transaction:
    from_addr: Address
    to: Address
    value: int
    data: Optional[bytes] = None
    nonce: int = 0
    signature: Optional[Signature] = None
    hash: Optional[Hash] = None


@dataclass
class Block:
    slot: int
    hash: Hash
    parent_hash: Hash
    proposer: Address
    transactions: List[Hash]
    state_root: Hash
    timestamp: int
    vrf_proof: Optional[bytes] = None


@dataclass
class Account:
    address: Address
    balance: int
    nonce: int
    code_hash: Optional[Hash] = None


@dataclass
class Validator:
    address: Address
    stake: int
    delegated_stake: int
    commission: int
    active: bool
    uptime: float


@dataclass
class Delegation:
    delegator: Address
    validator: Address
    amount: int
    rewards: int


ProposalStatus = Literal["active", "passed", "rejected", "executed"]


@dataclass
class Proposal:
    id: int
    proposer: Address
    title: str
    description: str
    votes_for: int
    votes_against: int
    status: ProposalStatus
    start_slot: int
    end_slot: int


@dataclass
class Vote:
    proposal_id: int
    voter: Address
    support: bool
    voting_power: int


JobStatus = Literal["pending", "assigned", "computing", "completed", "challenged", "settled"]


@dataclass
class AIJob:
    id: Hash
    creator: Address
    model_hash: Hash
    input_data: bytes
    aic_locked: int
    status: JobStatus
    provider: Optional[Address] = None
    result: Optional[bytes] = None
    vcr: Optional["VerifiableComputeReceipt"] = None


@dataclass
class VerifiableComputeReceipt:
    job_id: Hash
    provider: Address
    result: bytes
    execution_trace: Hash
    kzg_commitments: List[bytes]
    tee_attestation: bytes
    timestamp: int


@dataclass
class TransactionReceipt:
    transaction_hash: Hash
    block_hash: Hash
    block_slot: int
    from_addr: Address
    to: Address
    status: Literal["success", "failed"]
    gas_used: int
    logs: List[dict]

