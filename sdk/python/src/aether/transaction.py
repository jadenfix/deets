"""
Transaction building and signing
"""

import hashlib
import struct
from typing import Optional
from .keypair import Keypair
from .types import Transaction, Address, Hash


class TransactionBuilder:
    """Builder for creating transactions"""

    def __init__(self) -> None:
        self._from: Optional[Address] = None
        self._to: Optional[Address] = None
        self._value: Optional[int] = None
        self._data: Optional[bytes] = None
        self._nonce: Optional[int] = None

    def from_addr(self, address: Address) -> "TransactionBuilder":
        """Set sender address"""
        self._from = address
        return self

    def to(self, address: Address) -> "TransactionBuilder":
        """Set recipient address"""
        self._to = address
        return self

    def value(self, amount: int) -> "TransactionBuilder":
        """Set transfer amount"""
        self._value = amount
        return self

    def data(self, data: bytes) -> "TransactionBuilder":
        """Set transaction data"""
        self._data = data
        return self

    def nonce(self, nonce: int) -> "TransactionBuilder":
        """Set nonce"""
        self._nonce = nonce
        return self

    def build(self) -> Transaction:
        """Build unsigned transaction"""
        if self._from is None:
            raise ValueError("Transaction requires from address")
        if self._to is None:
            raise ValueError("Transaction requires to address")
        if self._value is None:
            raise ValueError("Transaction requires value")
        if self._nonce is None:
            raise ValueError("Transaction requires nonce")

        return Transaction(
            from_addr=self._from,
            to=self._to,
            value=self._value,
            data=self._data,
            nonce=self._nonce,
        )

    async def sign(self, keypair: Keypair) -> Transaction:
        """Sign and build transaction"""
        tx = self.build()

        tx_bytes = self._serialize(tx)
        hash_bytes = hashlib.sha256(tx_bytes).digest()
        hash_hex = "0x" + hash_bytes.hex()

        signature = keypair.sign(hash_bytes)

        tx.signature = signature
        tx.hash = hash_hex

        return tx

    @staticmethod
    def _serialize(tx: Transaction) -> bytes:
        """Serialize transaction for hashing/signing"""
        parts = [
            tx.from_addr.encode(),
            tx.to.encode(),
            struct.pack("<Q", tx.value),
            tx.data or b"",
            struct.pack("<I", tx.nonce),
        ]
        return b"".join(parts)

    @classmethod
    def transfer(
        cls, from_addr: Address, to: Address, amount: int, nonce: int
    ) -> "TransactionBuilder":
        """Helper: Create transfer transaction"""
        return cls().from_addr(from_addr).to(to).value(amount).nonce(nonce)

    @classmethod
    def call(
        cls,
        from_addr: Address,
        contract: Address,
        data: bytes,
        nonce: int,
        value: int = 0,
    ) -> "TransactionBuilder":
        """Helper: Create contract call transaction"""
        return (
            cls()
            .from_addr(from_addr)
            .to(contract)
            .value(value)
            .data(data)
            .nonce(nonce)
        )


class TransactionHelper:
    """Convenient wrapper for transaction operations"""

    @staticmethod
    async def create_transfer(
        keypair: Keypair, to: Address, amount: int, nonce: int
    ) -> Transaction:
        """Create and sign a simple transfer"""
        return await TransactionBuilder.transfer(
            keypair.address, to, amount, nonce
        ).sign(keypair)

    @staticmethod
    async def create_call(
        keypair: Keypair,
        contract: Address,
        data: bytes,
        nonce: int,
        value: int = 0,
    ) -> Transaction:
        """Create and sign a contract call"""
        return await TransactionBuilder.call(
            keypair.address, contract, data, nonce, value
        ).sign(keypair)

    @staticmethod
    def parse_hash(hash: str) -> Hash:
        """Parse transaction hash from hex"""
        if not hash.startswith("0x"):
            return "0x" + hash
        return hash

