// ============================================================================
// AETHER CRYPTO VRF - Verifiable Random Function for Leader Election
// ============================================================================
// PURPOSE: Provably random leader election without trusted randomness beacon
//
// ALGORITHM: ECVRF (Elliptic Curve Verifiable Random Function)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                       VRF SYSTEM                                  │
// ├──────────────────────────────────────────────────────────────────┤
// │  Epoch Randomness (η_e)  →  Slot Number  →  VRF Input            │
// │         ↓                                      ↓                  │
// │  Validator Secret Key  →  VRF Compute  →  (Output U, Proof π)    │
// │         ↓                                      ↓                  │
// │  U < Threshold?  →  Leader Election                               │
// │  π Valid?  →  Other Validators Verify  →  Accept/Reject Block    │
// └──────────────────────────────────────────────────────────────────┘
//
// VRF PROPERTIES:
// 1. Uniqueness: Only one valid output per (secret_key, input) pair
// 2. Verifiability: Anyone can verify output matches proof
// 3. Unpredictability: Output pseudorandom until revealed
//
// LEADER ELECTION:
// ```
// η_e = H(VRF_{winner_prev}(η_{e-1} || e))  // Epoch randomness
//
// For validator i in slot s:
//   input = η_e || slot_number || epoch
//   (U_i, π_i) = VRF(secret_key_i, input)
//   
//   threshold = τ * (stake_i / Σ stake)
//   if U_i < threshold:
//       // Validator i is leader
//       propose_block(proof=π_i)
// ```
//
// VERIFICATION:
// ```
// fn verify_leader(pubkey, slot, proof, epoch_randomness, stake_table) -> bool:
//     input = epoch_randomness || slot || epoch
//     U = vrf_verify(pubkey, input, proof)
//     
//     if U is None:
//         return false  // Invalid proof
//     
//     threshold = TAU * (stake_table[pubkey] / total_stake)
//     return U < threshold
// ```
//
// EPOCH RANDOMNESS UPDATE:
// At end of epoch e:
//   winners = collect_vrf_outputs_from_epoch_blocks()
//   η_{e+1} = H(combine(winners))
//
// SECURITY:
// - No single validator can predict future leaders
// - Cannot grind for favorable randomness (commitment scheme)
// - Slashing for invalid VRF proofs
//
// OUTPUTS:
// - (U, π) → Block proposal header
// - η_e → Next epoch's randomness seed
// - Verification results → Consensus accept/reject
// ============================================================================

pub mod vrf;
pub mod epoch_randomness;

pub use vrf::{VrfProof, VrfOutput};
pub use epoch_randomness::EpochRandomness;

