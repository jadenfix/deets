// ============================================================================
// AETHER CRYPTO KES - Key Evolving Signature Scheme
// ============================================================================
// PURPOSE: Limit damage from validator key compromise (forward security)
//
// ALGORITHM: KES (Key Evolving Signatures) - binary tree construction
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    KES KEY ROTATION                               │
// ├──────────────────────────────────────────────────────────────────┤
// │  Validator Identity Key (cold)  →  KES Master Key Generation     │
// │         ↓                                      ↓                  │
// │  Epoch Start  →  Derive KES Period Key  →  Delete Previous       │
// │         ↓                                      ↓                  │
// │  Block Signing  →  KES Period Signature  →  Include Period #     │
// │         ↓                                      ↓                  │
// │  Verification  →  Check Period Valid  →  Accept/Reject           │
// └──────────────────────────────────────────────────────────────────┘
//
// KES PROPERTIES:
// 1. Forward Security: Compromise of key_t doesn't reveal key_{t-1}
// 2. One-way Evolution: Cannot derive future keys from current key
// 3. Bounded Lifetime: Master key good for N periods, then regenerate
//
// KEY EVOLUTION:
// ```
// Master key K_0 generates binary tree of depth d:
//   Period 0: K_0
//   Period 1: K_1 = evolve(K_0)
//   Period 2: K_2 = evolve(K_1)
//   ...
//   Period 2^d - 1: K_{2^d - 1}
//
// After period t, delete K_0 ... K_{t-1}
// ```
//
// SIGNATURE:
// ```
// struct KesSignature:
//     period: u32
//     signature: Signature
//     auxiliary_data: Vec<u8>  // Merkle path for verification
//
// fn kes_sign(kes_key, period, message) -> KesSignature:
//     if period != kes_key.current_period:
//         kes_key.evolve_to(period)
//     
//     sig = sign(kes_key.current_key, message)
//     path = kes_key.merkle_path()
//     
//     return KesSignature {
//         period: period,
//         signature: sig,
//         auxiliary_data: path
//     }
// ```
//
// VERIFICATION:
// ```
// fn kes_verify(pubkey, message, kes_sig) -> bool:
//     // Check period is valid
//     if kes_sig.period > max_period:
//         return false
//     
//     // Derive period pubkey from master pubkey
//     period_pubkey = derive_period_key(pubkey, kes_sig.period, kes_sig.auxiliary_data)
//     
//     // Verify signature
//     return verify(period_pubkey, message, kes_sig.signature)
// ```
//
// OPERATIONAL:
// - Period = epoch (e.g., 6 hours)
// - Master key lifetime: ~90 days (360 periods at 6hr epochs)
// - Automated rotation via remote signer
// - Cold wallet regenerates master key quarterly
//
// SECURITY:
// - If hot wallet compromised at period t, attacker cannot:
//   * Sign for period < t (forward security)
//   * Derive keys for period > t (one-way)
// - Limits blast radius of compromise
//
// OUTPUTS:
// - KES signatures → Block headers
// - Period verification → Slashing prevention
// - Rotation events → Monitoring alerts
// ============================================================================

pub mod evolution;
pub mod signature;

pub use evolution::KesKey;
pub use signature::KesSignature;

