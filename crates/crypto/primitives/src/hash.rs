use blake3::Hasher as Blake3Hasher;
use sha2::{Digest, Sha256};

#[inline]
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[inline]
#[must_use]
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[must_use]
pub fn hash_multiple(chunks: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let data = b"hello world";
        let hash = sha256(data);
        assert_eq!(hash.len(), 32);

        // Deterministic
        let hash2 = sha256(data);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_blake3() {
        let data = b"hello world";
        let hash = blake3_hash(data);
        assert_eq!(hash.len(), 32);

        // Deterministic
        let hash2 = blake3_hash(data);
        assert_eq!(hash, hash2);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// sha256 is deterministic.
        #[test]
        fn sha256_deterministic(data in prop::collection::vec(any::<u8>(), 0..256)) {
            prop_assert_eq!(sha256(&data), sha256(&data));
        }

        /// sha256 output is always 32 bytes.
        #[test]
        fn sha256_output_len(data in prop::collection::vec(any::<u8>(), 0..256)) {
            prop_assert_eq!(sha256(&data).len(), 32);
        }

        /// blake3_hash is deterministic.
        #[test]
        fn blake3_deterministic(data in prop::collection::vec(any::<u8>(), 0..256)) {
            prop_assert_eq!(blake3_hash(&data), blake3_hash(&data));
        }

        /// blake3_hash output is always 32 bytes.
        #[test]
        fn blake3_output_len(data in prop::collection::vec(any::<u8>(), 0..256)) {
            prop_assert_eq!(blake3_hash(&data).len(), 32);
        }

        /// Different inputs produce different sha256 hashes (collision resistance spot-check).
        #[test]
        fn sha256_collision_resistance(
            a in prop::collection::vec(any::<u8>(), 1..128),
            b in prop::collection::vec(any::<u8>(), 1..128),
        ) {
            prop_assume!(a != b);
            prop_assert_ne!(sha256(&a), sha256(&b));
        }

        /// Different inputs produce different blake3 hashes (collision resistance spot-check).
        #[test]
        fn blake3_collision_resistance(
            a in prop::collection::vec(any::<u8>(), 1..128),
            b in prop::collection::vec(any::<u8>(), 1..128),
        ) {
            prop_assume!(a != b);
            prop_assert_ne!(blake3_hash(&a), blake3_hash(&b));
        }

        /// hash_multiple([a, b]) == hash_multiple([a, b]) (deterministic).
        #[test]
        fn hash_multiple_deterministic(
            a in prop::collection::vec(any::<u8>(), 1..64),
            b in prop::collection::vec(any::<u8>(), 1..64),
        ) {
            let h1 = hash_multiple(&[&a, &b]);
            let h2 = hash_multiple(&[&a, &b]);
            prop_assert_eq!(h1, h2);
        }

        /// hash_multiple([a, b]) != hash_multiple([b, a]) when a != b (order sensitivity).
        #[test]
        fn hash_multiple_order_sensitive(
            a in prop::collection::vec(any::<u8>(), 1..32),
            b in prop::collection::vec(any::<u8>(), 1..32),
        ) {
            prop_assume!(a != b);
            let h_ab = hash_multiple(&[&a, &b]);
            let h_ba = hash_multiple(&[&b, &a]);
            prop_assert_ne!(h_ab, h_ba, "hash_multiple must be order-sensitive");
        }

        /// hash_multiple([data]) equals sha256(data) — single-chunk consistency.
        #[test]
        fn hash_multiple_single_matches_sha256(data in prop::collection::vec(any::<u8>(), 1..128)) {
            prop_assert_eq!(hash_multiple(&[&data]), sha256(&data));
        }
    }
}
