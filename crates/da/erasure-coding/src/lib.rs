// ============================================================================
// AETHER ERASURE CODING - Reed-Solomon for Data Availability
// ============================================================================
// PURPOSE: Encode blocks for redundant distribution and fault tolerance
//
// ALGORITHM: Reed-Solomon RS(n, k) error correction codes
//
// PARAMETERS:
// - k: Number of data shards (original pieces)
// - r: Number of parity shards (redundancy)
// - n = k + r: Total shards
// - Any k of n shards can reconstruct original data
//
// EXAMPLE: RS(12, 10)
// - 10 data shards (original block split into 10 pieces)
// - 2 parity shards (computed from data shards)
// - Can lose any 2 shards and still reconstruct
// - Overhead: 20% (2/10)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    ERASURE CODING                                 │
// ├──────────────────────────────────────────────────────────────────┤
// │  Block Data  →  Split into k Chunks  →  Encode to n Shards       │
// │         ↓                                    ↓                    │
// │  Distribute n Shards  →  Turbine Broadcast                        │
// │         ↓                                    ↓                    │
// │  Receive ≥k Shards  →  Decode  →  Reconstruct Original Block     │
// └──────────────────────────────────────────────────────────────────┘
//
// PSEUDOCODE:
// ```
// struct ReedSolomon:
//     k: usize  // Data shards
//     r: usize  // Parity shards
//     encoder: RSEncoder
//
// fn encode(data: &[u8]) -> Vec<Vec<u8>>:
//     n = k + r
//     chunk_size = (data.len() + k - 1) / k
//
//     // Split data into k chunks
//     data_shards = split_into_chunks(data, k, chunk_size)
//
//     // Compute r parity shards
//     parity_shards = encoder.encode_parity(data_shards)
//
//     // Return all n shards
//     return data_shards + parity_shards
//
// fn decode(shards: Vec<(usize, Vec<u8>)>) -> Result<Vec<u8>>:
//     if shards.len() < k:
//         return Err("insufficient shards")
//
//     // Use any k shards to reconstruct all data shards
//     reconstructed = encoder.reconstruct(shards[0..k])
//
//     // Concatenate data shards
//     return join(reconstructed)
// ```
//
// MATH (simplified):
// Data vector D = [d1, d2, ..., dk]
// Generator matrix G (k × n) encodes:
//   Codeword C = D × G = [d1, d2, ..., dk, p1, p2, ..., pr]
//
// Any k columns of G form invertible matrix → can solve for D
//
// PERFORMANCE:
// - Encoding: O(k × r) operations per chunk
// - Decoding: O(k^2) for Gaussian elimination
// - 2MB block, k=10, r=2: ~5ms on modern CPU
//
// OUTPUTS:
// - Encoded shards → Turbine broadcaster
// - Reconstructed data → Block validator
// ============================================================================

pub mod decoder;
pub mod encoder;

pub use decoder::ReedSolomonDecoder;
pub use encoder::ReedSolomonEncoder;
