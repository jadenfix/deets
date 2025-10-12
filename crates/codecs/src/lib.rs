// ============================================================================
// AETHER CODECS - Binary Encoding/Decoding
// ============================================================================
// PURPOSE: Efficient, deterministic serialization for network & storage
//
// ENCODINGS:
// - Borsh: Compact binary (for on-chain programs)
// - Bincode: Fast binary (for network messages)
//
// DETERMINISM:
// - Canonical ordering (sorted maps)
// - No floating point (use fixed-point)
// - Consistent byte representation
// ============================================================================

pub mod borsh_codec;
pub mod bincode_codec;

pub use borsh_codec::{encode_borsh, decode_borsh};
pub use bincode_codec::{encode_bincode, decode_bincode};

