use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::error::{KesError, Result};
use crate::signature::{KesSignature, KesVerificationKey};

/// Key-Evolving Signature (KES) scheme using a binary tree of Ed25519 keypairs.
///
/// Based on the MMM (Malkin-Micciancio-Miner) sum composition construction
/// used in Cardano's Praos protocol.
///
/// The key tree has `depth` levels, supporting `2^depth` time periods.
/// At each period, only one leaf keypair is active. When the key evolves
/// to the next period, the old leaf's secret key is securely erased,
/// providing forward secrecy: compromise of the current key cannot forge
/// signatures for past periods.
///
/// Signature structure:
/// - Ed25519 signature from the active leaf keypair (64 bytes)
/// - Authentication path: sibling hashes from leaf to root (depth * 32 bytes)
/// - Active leaf's public key (32 bytes)
///
/// Verification:
/// 1. Verify the Ed25519 signature against the leaf public key
/// 2. Reconstruct the Merkle root from the leaf public key hash + auth path
/// 3. Compare against the stored root (verification key)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KesKey {
    /// All leaf signing keys. Evolved (erased) keys are set to None.
    leaves: Vec<Option<[u8; 32]>>,
    /// All leaf public keys (always available for auth path computation).
    leaf_pubkeys: Vec<[u8; 32]>,
    /// Merkle root of the leaf public key hashes.
    root: [u8; 32],
    /// Current period (monotonically increasing).
    current_period: u32,
    /// Tree depth (supports 2^depth periods).
    depth: u32,
}

impl KesKey {
    /// Generate a new KES key supporting `max_periods` time periods.
    /// `max_periods` is rounded up to the next power of 2.
    pub fn generate(max_periods: u32) -> Self {
        let max_periods = max_periods.max(2);
        let depth = (max_periods as f64).log2().ceil() as u32;
        let num_leaves = 1u32 << depth;

        let mut leaves = Vec::with_capacity(num_leaves as usize);
        let mut leaf_pubkeys = Vec::with_capacity(num_leaves as usize);

        for _ in 0..num_leaves {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key = signing_key.verifying_key();
            leaves.push(Some(signing_key.to_bytes()));
            leaf_pubkeys.push(verifying_key.to_bytes());
        }

        let root = compute_merkle_root(&leaf_pubkeys);

        KesKey {
            leaves,
            leaf_pubkeys,
            root,
            current_period: 0,
            depth,
        }
    }

    /// Create a KES key from an explicit seed (deterministic).
    pub fn from_seed(seed: [u8; 32], max_periods: u32) -> Self {
        let max_periods = max_periods.max(2);
        let depth = (max_periods as f64).log2().ceil() as u32;
        let num_leaves = 1u32 << depth;

        let mut leaves = Vec::with_capacity(num_leaves as usize);
        let mut leaf_pubkeys = Vec::with_capacity(num_leaves as usize);

        for i in 0..num_leaves {
            // Derive each leaf's secret deterministically from seed
            let mut hasher = Sha256::new();
            hasher.update(seed);
            hasher.update(b"kes-leaf");
            hasher.update(i.to_le_bytes());
            let leaf_seed: [u8; 32] = hasher.finalize().into();

            let signing_key = SigningKey::from_bytes(&leaf_seed);
            let verifying_key = signing_key.verifying_key();
            leaves.push(Some(leaf_seed));
            leaf_pubkeys.push(verifying_key.to_bytes());
        }

        let root = compute_merkle_root(&leaf_pubkeys);

        KesKey {
            leaves,
            leaf_pubkeys,
            root,
            current_period: 0,
            depth,
        }
    }

    /// Maximum number of supported periods.
    pub fn max_periods(&self) -> u32 {
        1u32 << self.depth
    }

    /// Current evolved period.
    pub fn current_period(&self) -> u32 {
        self.current_period
    }

    /// Public verification key (Merkle root + metadata).
    pub fn verification_key(&self) -> KesVerificationKey {
        KesVerificationKey::new(self.root, self.max_periods())
    }

    /// Sign a message at the given period.
    ///
    /// The period must be >= current_period (forward only).
    /// Signing at a period automatically evolves the key to that period,
    /// erasing all leaf keys for periods < `period`.
    pub fn sign(&mut self, period: u32, message: &[u8]) -> Result<KesSignature> {
        if period >= self.max_periods() {
            return Err(KesError::PeriodOutOfRange {
                requested: period,
                max_periods: self.max_periods(),
            });
        }

        if period < self.current_period {
            return Err(KesError::PeriodRegression {
                current: self.current_period,
                requested: period,
            });
        }

        // Evolve: erase all keys for periods before `period`
        self.evolve_to(period);

        // Get the active leaf's signing key
        let leaf_secret = self.leaves[period as usize]
            .as_ref()
            .ok_or(KesError::KeyErased { period })?;

        let signing_key = SigningKey::from_bytes(leaf_secret);
        let ed_signature = signing_key.sign(message);

        // Build authentication path (sibling hashes from leaf to root)
        let auth_path = compute_auth_path(&self.leaf_pubkeys, period as usize);

        Ok(KesSignature {
            period,
            signature: ed_signature.to_bytes().to_vec(),
            leaf_pubkey: self.leaf_pubkeys[period as usize],
            auth_path,
        })
    }

    /// Evolve the key to a target period, securely erasing past keys.
    fn evolve_to(&mut self, target_period: u32) {
        for i in self.current_period..target_period {
            if let Some(ref mut key_bytes) = self.leaves[i as usize] {
                key_bytes.zeroize();
            }
            self.leaves[i as usize] = None;
        }
        self.current_period = target_period;
    }
}

impl Drop for KesKey {
    fn drop(&mut self) {
        // Zeroize all remaining secret key material
        for key_bytes in self.leaves.iter_mut().flatten() {
            key_bytes.zeroize();
        }
    }
}

/// Compute the Merkle root from leaf public keys.
fn compute_merkle_root(leaf_pubkeys: &[[u8; 32]]) -> [u8; 32] {
    // Hash each leaf public key
    let mut current_level: Vec<[u8; 32]> = leaf_pubkeys
        .iter()
        .map(|pk| {
            let mut h = Sha256::new();
            h.update([0x00]);
            h.update(pk);
            h.finalize().into()
        })
        .collect();

    // Build tree bottom-up
    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity(current_level.len() / 2);
        for pair in current_level.chunks(2) {
            let mut h = Sha256::new();
            h.update([0x01]);
            h.update(pair[0]);
            if pair.len() > 1 {
                h.update(pair[1]);
            } else {
                h.update(pair[0]); // duplicate if odd
            }
            next_level.push(h.finalize().into());
        }
        current_level = next_level;
    }

    current_level[0]
}

/// Compute the authentication path (sibling hashes) for a given leaf index.
fn compute_auth_path(leaf_pubkeys: &[[u8; 32]], leaf_index: usize) -> Vec<[u8; 32]> {
    // Hash each leaf public key
    let mut current_level: Vec<[u8; 32]> = leaf_pubkeys
        .iter()
        .map(|pk| {
            let mut h = Sha256::new();
            h.update([0x00]);
            h.update(pk);
            h.finalize().into()
        })
        .collect();

    let mut path = Vec::new();
    let mut idx = leaf_index;

    while current_level.len() > 1 {
        // Sibling index
        let sibling_idx = idx ^ 1;
        if sibling_idx < current_level.len() {
            path.push(current_level[sibling_idx]);
        } else {
            path.push(current_level[idx]); // duplicate if no sibling
        }

        // Build next level
        let mut next_level = Vec::with_capacity(current_level.len() / 2);
        for pair in current_level.chunks(2) {
            let mut h = Sha256::new();
            h.update([0x01]);
            h.update(pair[0]);
            if pair.len() > 1 {
                h.update(pair[1]);
            } else {
                h.update(pair[0]);
            }
            next_level.push(h.finalize().into());
        }

        current_level = next_level;
        idx /= 2;
    }

    path
}

/// Verify a Merkle authentication path from a leaf to the expected root.
pub(crate) fn verify_auth_path(
    leaf_pubkey: &[u8; 32],
    leaf_index: usize,
    auth_path: &[[u8; 32]],
    expected_root: &[u8; 32],
) -> bool {
    if auth_path.is_empty() {
        return false;
    }
    let max_leaves = 1usize << auth_path.len();
    if leaf_index >= max_leaves {
        return false;
    }

    // Hash the leaf public key
    let mut current_hash: [u8; 32] = {
        let mut h = Sha256::new();
        h.update([0x00]);
        h.update(leaf_pubkey);
        h.finalize().into()
    };

    let mut idx = leaf_index;
    for sibling in auth_path {
        let mut h = Sha256::new();
        h.update([0x01]);
        if idx % 2 == 0 {
            h.update(current_hash);
            h.update(sibling);
        } else {
            h.update(sibling);
            h.update(current_hash);
        }
        current_hash = h.finalize().into();
        idx /= 2;
    }

    bool::from(current_hash.ct_eq(expected_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kes_generates_and_signs() {
        let mut key = KesKey::generate(16);
        let vk = key.verification_key();

        let sig = key.sign(0, b"hello world").unwrap();
        assert!(sig.verify(&vk, b"hello world"));
        assert!(!sig.verify(&vk, b"different"));
        assert_eq!(key.current_period(), 0);
    }

    #[test]
    fn test_kes_monotonic_period() {
        let mut key = KesKey::generate(4);
        key.sign(1, b"test").unwrap();
        let err = key.sign(0, b"regress").unwrap_err();
        assert_eq!(
            err,
            KesError::PeriodRegression {
                current: 1,
                requested: 0,
            }
        );
    }

    #[test]
    fn test_kes_period_bounds_check() {
        let mut key = KesKey::generate(4);
        // max_periods is rounded up to 4 (2^2)
        key.sign(0, b"test").unwrap();
        let err = key.sign(4, b"oob").unwrap_err();
        assert_eq!(
            err,
            KesError::PeriodOutOfRange {
                requested: 4,
                max_periods: 4,
            }
        );
    }

    #[test]
    fn test_kes_forward_secrecy_erases_keys() {
        let mut key = KesKey::generate(4);
        let vk = key.verification_key();

        // Sign at period 0
        let sig0 = key.sign(0, b"period 0").unwrap();
        assert!(sig0.verify(&vk, b"period 0"));

        // Evolve to period 2 — periods 0 and 1 are erased
        let sig2 = key.sign(2, b"period 2").unwrap();
        assert!(sig2.verify(&vk, b"period 2"));

        // Periods 0 and 1 keys should be None
        assert!(key.leaves[0].is_none(), "period 0 key should be erased");
        assert!(key.leaves[1].is_none(), "period 1 key should be erased");
        // Period 2 key should still exist
        assert!(key.leaves[2].is_some(), "period 2 key should exist");
    }

    #[test]
    fn test_kes_all_periods_sign_and_verify() {
        let mut key = KesKey::generate(8);
        let vk = key.verification_key();

        for period in 0..key.max_periods() {
            let msg = format!("message for period {}", period);
            let sig = key.sign(period, msg.as_bytes()).unwrap();
            assert!(
                sig.verify(&vk, msg.as_bytes()),
                "period {} should verify",
                period
            );
        }
    }

    #[test]
    fn test_kes_wrong_message_fails_verification() {
        let mut key = KesKey::generate(4);
        let vk = key.verification_key();

        let sig = key.sign(0, b"correct").unwrap();
        assert!(!sig.verify(&vk, b"wrong"));
    }

    #[test]
    fn test_kes_wrong_verification_key_fails() {
        let mut key1 = KesKey::generate(4);
        let key2 = KesKey::generate(4);

        let sig = key1.sign(0, b"test").unwrap();
        let wrong_vk = key2.verification_key();

        assert!(!sig.verify(&wrong_vk, b"test"));
    }

    #[test]
    fn test_kes_deterministic_from_seed() {
        let seed = [42u8; 32];
        let key1 = KesKey::from_seed(seed, 8);
        let key2 = KesKey::from_seed(seed, 8);

        assert_eq!(key1.root, key2.root);
        assert_eq!(key1.verification_key(), key2.verification_key());
    }

    #[test]
    fn test_kes_merkle_root_consistency() {
        let key = KesKey::generate(8);
        let vk = key.verification_key();

        // Recompute root manually
        let root = compute_merkle_root(&key.leaf_pubkeys);
        assert_eq!(root, vk.root());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Small period counts (2–16) for fast proptest runs.
    fn arb_max_periods() -> impl Strategy<Value = u32> {
        2u32..=16u32
    }

    proptest! {
        /// sign/verify roundtrip: signing a message at any valid period always verifies.
        #[test]
        fn sign_verify_roundtrip(
            seed in prop::array::uniform32(any::<u8>()),
            max_periods in arb_max_periods(),
            message in prop::collection::vec(any::<u8>(), 0..128),
        ) {
            let mut key = KesKey::from_seed(seed, max_periods);
            let vk = key.verification_key();
            let actual_max = key.max_periods();
            // Sign at period 0 (safe; always valid)
            let sig = key.sign(0, &message).unwrap();
            prop_assert!(sig.verify(&vk, &message),
                "sign/verify roundtrip must succeed (max_periods={})", actual_max);
        }

        /// Wrong message fails verification.
        #[test]
        fn wrong_message_fails(
            seed in prop::array::uniform32(any::<u8>()),
            msg1 in prop::collection::vec(any::<u8>(), 1..64),
            msg2 in prop::collection::vec(any::<u8>(), 1..64),
        ) {
            prop_assume!(msg1 != msg2);
            let mut key = KesKey::from_seed(seed, 4);
            let vk = key.verification_key();
            let sig = key.sign(0, &msg1).unwrap();
            prop_assert!(!sig.verify(&vk, &msg2),
                "signature on msg1 must not verify against msg2");
        }

        /// Wrong verification key fails.
        #[test]
        fn wrong_vk_fails(
            seed1 in prop::array::uniform32(any::<u8>()),
            seed2 in prop::array::uniform32(any::<u8>()),
            message in prop::collection::vec(any::<u8>(), 0..64),
        ) {
            prop_assume!(seed1 != seed2);
            let mut key1 = KesKey::from_seed(seed1, 4);
            let key2 = KesKey::from_seed(seed2, 4);
            let wrong_vk = key2.verification_key();
            let sig = key1.sign(0, &message).unwrap();
            prop_assert!(!sig.verify(&wrong_vk, &message),
                "signature with key1 must not verify under key2's vk");
        }

        /// Period regression is rejected: sign at p then attempt p-1 fails.
        #[test]
        fn period_regression_rejected(
            seed in prop::array::uniform32(any::<u8>()),
            period in 1u32..8u32,
        ) {
            let mut key = KesKey::from_seed(seed, 16);
            key.sign(period, b"advance").unwrap();
            let result = key.sign(period - 1, b"regress");
            prop_assert!(result.is_err(), "regression to period {} from {} must fail",
                period - 1, period);
        }

        /// Period out-of-range is rejected.
        #[test]
        fn period_out_of_range_rejected(
            seed in prop::array::uniform32(any::<u8>()),
            max_periods in arb_max_periods(),
        ) {
            let mut key = KesKey::from_seed(seed, max_periods);
            let actual_max = key.max_periods();
            let result = key.sign(actual_max, b"oob");
            prop_assert!(result.is_err(), "sign at max_periods={} must fail (out-of-range)", actual_max);
        }

        /// Forward secrecy: past leaf keys are None after evolving forward.
        #[test]
        fn forward_secrecy_erases_past_keys(
            seed in prop::array::uniform32(any::<u8>()),
            target in 1u32..8u32,
        ) {
            let mut key = KesKey::from_seed(seed, 16);
            key.sign(target, b"evolve").unwrap();
            for i in 0..target {
                prop_assert!(key.leaves[i as usize].is_none(),
                    "leaf key at period {} must be erased after evolving to {}", i, target);
            }
            // Current period's key must still be present
            prop_assert!(key.leaves[target as usize].is_some(),
                "leaf key at current period {} must still exist", target);
        }

        /// Deterministic key generation: same seed always gives same verification key.
        #[test]
        fn deterministic_from_seed(
            seed in prop::array::uniform32(any::<u8>()),
            max_periods in arb_max_periods(),
        ) {
            let k1 = KesKey::from_seed(seed, max_periods);
            let k2 = KesKey::from_seed(seed, max_periods);
            prop_assert_eq!(k1.verification_key(), k2.verification_key(),
                "same seed must produce same verification key");
        }

        /// Tampered leaf_pubkey in signature fails verification.
        #[test]
        fn tampered_leaf_pubkey_fails(
            seed in prop::array::uniform32(any::<u8>()),
            tamper_byte in any::<u8>(),
            message in prop::collection::vec(any::<u8>(), 1..64),
        ) {
            let mut key = KesKey::from_seed(seed, 4);
            let vk = key.verification_key();
            let mut sig = key.sign(0, &message).unwrap();
            // Flip one byte in the leaf pubkey
            sig.leaf_pubkey[0] = sig.leaf_pubkey[0].wrapping_add(tamper_byte.saturating_add(1));
            prop_assert!(!sig.verify(&vk, &message),
                "tampered leaf_pubkey must fail verification");
        }

        /// max_periods() is always a power of two >= 2.
        #[test]
        fn max_periods_is_power_of_two(
            seed in prop::array::uniform32(any::<u8>()),
            max_periods in 2u32..=32u32,
        ) {
            let key = KesKey::from_seed(seed, max_periods);
            let actual = key.max_periods();
            prop_assert!(actual >= 2, "max_periods must be >= 2");
            prop_assert!(actual.is_power_of_two(), "max_periods must be a power of two");
            prop_assert!(actual >= max_periods, "max_periods must cover requested count");
        }
    }
}
