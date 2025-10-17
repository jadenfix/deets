"""
Aether SDK - Python

Official Python SDK for Aether Blockchain with async/await support.
"""

from .client import AetherClient
from .keypair import Keypair
from .transaction import Transaction, TransactionBuilder
from .staking import StakingHelper
from .governance import GovernanceHelper
from .ai import AIJobHelper, ModelHelper
from .types import *

__version__ = "0.1.0"

__all__ = [
    "AetherClient",
    "Keypair",
    "Transaction",
    "TransactionBuilder",
    "StakingHelper",
    "GovernanceHelper",
    "AIJobHelper",
    "ModelHelper",
    "__version__",
]

