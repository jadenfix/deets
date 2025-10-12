// ============================================================================
// AETHER SHREDS - Block Fragment Data Structures
// ============================================================================
// PURPOSE: Wire format for erasure-coded block pieces
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    SHRED STRUCTURE                                │
// ├──────────────────────────────────────────────────────────────────┤
// │  Block  →  Erasure Encode  →  Shreds (with metadata)             │
// │         ↓                              ↓                          │
// │  Serialize Shred  →  Gossipsub 'shred' topic  →  Peers           │
// │         ↓                              ↓                          │
// │  Deserialize  →  Validate  →  Reconstruct Buffer                 │
// └──────────────────────────────────────────────────────────────────┘
//
// SHRED FORMAT:
// ```
// struct Shred:
//     variant: ShredVariant  // Data or Parity
//     slot: u64
//     index: u32  // Position in erasure set
//     version: u16
//     fec_set_index: u32  // Which FEC set this belongs to
//     payload: Vec<u8>  // Actual data chunk
//     signature: Signature  // Leader signature
// ```
//
// PSEUDOCODE:
// ```
// enum ShredVariant:
//     Data   // Original block data
//     Parity // Erasure-coded parity
//
// fn create_shreds(block, slot, leader_key):
//     // Split block into packets
//     packets = split_block(block, MAX_SHRED_PAYLOAD)
//     
//     shreds = []
//     for (i, packet) in enumerate(packets):
//         shred = Shred {
//             variant: Data,
//             slot: slot,
//             index: i,
//             version: PROTOCOL_VERSION,
//             fec_set_index: 0,
//             payload: packet,
//             signature: sign(leader_key, packet)
//         }
//         shreds.push(shred)
//     
//     return shreds
//
// fn validate_shred(shred, leader_pubkey) -> bool:
//     // Check signature
//     if !verify(leader_pubkey, shred.payload, shred.signature):
//         return false
//     
//     // Check slot is recent
//     if shred.slot < current_slot - MAX_SLOT_AGE:
//         return false
//     
//     return true
//
// fn reconstruct_block(shreds) -> Option<Block>:
//     // Group by FEC set
//     fec_set = group_by_fec_set(shreds)
//     
//     // Need k data shreds or reconstruct from k of n
//     if fec_set.data_shreds.len() >= k:
//         return assemble_data_shreds(fec_set.data_shreds)
//     
//     if fec_set.total_shreds() >= k:
//         return erasure_decode(fec_set.all_shreds())
//     
//     return None  // Insufficient shreds
// ```
//
// WIRE PROTOCOL:
// - Shreds gossipped on 'shred' topic
// - ~170KB per shred for 2MB block / 12 shreds
// - Signature verification on receipt
// - Deduplication by (slot, index)
//
// OUTPUTS:
// - Serialized shreds → Gossipsub
// - Validated shreds → Reconstructor
// - Reconstruction status → Repair requests
// ============================================================================

pub mod shred;
pub mod serialization;
pub mod validation;

pub use shred::Shred;

