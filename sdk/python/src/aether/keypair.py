"""
Keypair management for Aether

Ed25519 keypair generation and signing
"""

import hashlib
import nacl.signing
import nacl.encoding
from typing import Tuple
from .types import Address, Signature


class Keypair:
    """Ed25519 keypair for signing transactions"""

    def __init__(self, signing_key: nacl.signing.SigningKey):
        self._signing_key = signing_key
        self._verify_key = signing_key.verify_key
        self.public_key = bytes(self._verify_key)
        self.secret_key = bytes(signing_key)
        self.address = self.public_key_to_address(self.public_key)

    @classmethod
    def generate(cls) -> "Keypair":
        """Generate a new random keypair"""
        signing_key = nacl.signing.SigningKey.generate()
        return cls(signing_key)

    @classmethod
    def from_secret_key(cls, secret_key: bytes) -> "Keypair":
        """Create keypair from existing secret key"""
        signing_key = nacl.signing.SigningKey(secret_key)
        return cls(signing_key)

    @classmethod
    def from_seed(cls, seed: str) -> "Keypair":
        """Create keypair from seed phrase (deterministic)"""
        seed_bytes = seed.encode('utf-8')
        secret_key = hashlib.sha256(seed_bytes).digest()
        return cls.from_secret_key(secret_key)

    @classmethod
    def from_secret_key_hex(cls, hex_str: str) -> "Keypair":
        """Create keypair from hex-encoded secret key"""
        secret_key = bytes.fromhex(hex_str)
        return cls.from_secret_key(secret_key)

    def sign(self, message: bytes) -> Signature:
        """Sign a message"""
        signed = self._signing_key.sign(message)
        return signed.signature.hex()

    @staticmethod
    def verify(signature: Signature, message: bytes, public_key: bytes) -> bool:
        """Verify a signature"""
        try:
            verify_key = nacl.signing.VerifyKey(public_key)
            sig_bytes = bytes.fromhex(signature)
            verify_key.verify(message, sig_bytes)
            return True
        except:
            return False

    @staticmethod
    def public_key_to_address(public_key: bytes) -> Address:
        """Convert public key to Aether address"""
        hash_bytes = hashlib.sha256(public_key).digest()
        address_bytes = hash_bytes[-20:]
        return "0x" + address_bytes.hex()

    def to_secret_key_hex(self) -> str:
        """Export secret key as hex string"""
        return self.secret_key.hex()

    def to_public_key_hex(self) -> str:
        """Export public key as hex string"""
        return self.public_key.hex()

