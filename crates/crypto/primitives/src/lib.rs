// ============================================================================
// AETHER CRYPTO PRIMITIVES - Core Cryptographic Functions
// ============================================================================
// PURPOSE: Basic crypto operations (hashing, signing) used across the system
//
// CRYPTOGRAPHIC SUITE:
// - Signing: Ed25519 (transaction signatures)
// - Hashing: SHA-256 (general), BLAKE3 (PoH-style sequencing)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    CRYPTO PRIMITIVES                              │
// ├──────────────────────────────────────────────────────────────────┤
// │  Transaction  →  Ed25519 Sign/Verify  →  Mempool Validation      │
// │  Block Data  →  SHA-256 Hashing  →  Merkle Tree  →  State Root   │
// │  PoH Chain  →  BLAKE3 Fast Hash  →  Timestamp Proof              │
// └──────────────────────────────────────────────────────────────────┘
//
// OUTPUTS:
// - Signatures → Transaction authentication
// - Hashes → Merkle trees, content addressing
// - Public keys → Address derivation
// ============================================================================

pub mod ed25519;
pub mod hash;
pub mod keypair;

pub use ed25519::{verify, Keypair as Ed25519Keypair};
pub use hash::{blake3_hash, hash_multiple, sha256};
pub use keypair::Keypair;
