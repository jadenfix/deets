// ============================================================================
// AETHER CRYPTO VRF - Verifiable Random Function
// ============================================================================
// PURPOSE: ECVRF for slot leader election in VRF-PoS consensus
//
// ALGORITHM: ECVRF-EDWARDS25519-SHA512-ELL2 (IETF draft-irtf-cfrg-vrf)
//
// USAGE:
// 1. Each validator has a VRF keypair
// 2. For each slot, evaluate VRF(secret_key, epoch_randomness || slot)
// 3. If VRF_output < threshold * (validator_stake / total_stake), validator is leader
// 4. Leader includes VRF proof in block header
// 5. Others verify the proof to confirm leader legitimacy
//
// SECURITY:
// - Unpredictable: VRF output is pseudorandom
// - Verifiable: Anyone can verify the proof
// - Non-interactive: No communication needed for proof generation
// - Grinding-resistant: Cannot selectively choose favorable outputs
//
// EPOCH RANDOMNESS:
// η_e = H(VRF_i(η_{e-1} || e)) - chained randomness across epochs
// ============================================================================

pub mod ecvrf;

pub use ecvrf::{check_leader_eligibility, output_to_value, verify_proof, VrfKeypair, VrfProof};
