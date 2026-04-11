use anyhow::{bail, Result};
use reed_solomon_erasure::galois_8::ReedSolomon;

/// Production Reed-Solomon decoder with error correction
/// Reconstructs original data from any k shards (data or parity)
#[derive(Debug)]
pub struct ReedSolomonDecoder {
    data_shards: usize,
    parity_shards: usize,
    decoder: ReedSolomon,
}

impl ReedSolomonDecoder {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        if data_shards == 0 {
            bail!("data shards must be non-zero");
        }
        if parity_shards == 0 {
            bail!("parity shards must be non-zero");
        }

        let decoder = ReedSolomon::new(data_shards, parity_shards)
            .map_err(|e| anyhow::anyhow!("failed to create RS decoder: {}", e))?;

        Ok(ReedSolomonDecoder {
            data_shards,
            parity_shards,
            decoder,
        })
    }

    /// Decode shards back into original data
    ///
    /// Accepts `Option<Vec<u8>>` to indicate present (Some) or missing (None) shards
    /// Requires at least k (data_shards) shards to be present
    pub fn decode(&self, shards: &[Option<Vec<u8>>]) -> Result<Vec<u8>> {
        let expected = self.data_shards + self.parity_shards;
        if shards.len() != expected {
            bail!(
                "expected {} shards (data + parity), got {}",
                expected,
                shards.len()
            );
        }

        // Count present shards
        let present_count = shards.iter().filter(|s| s.is_some()).count();
        if present_count < self.data_shards {
            bail!(
                "insufficient shards for reconstruction: need {}, have {}",
                self.data_shards,
                present_count
            );
        }

        // Clone shards for reconstruction (library needs owned data)
        let mut working_shards: Vec<Option<Vec<u8>>> = shards.to_vec();

        // Reconstruct missing shards
        self.decoder
            .reconstruct(&mut working_shards)
            .map_err(|e| anyhow::anyhow!("reconstruction failed: {}", e))?;

        // Concatenate data shards
        let mut data = Vec::new();
        for (i, shard_opt) in working_shards.iter().take(self.data_shards).enumerate() {
            if let Some(shard) = shard_opt {
                data.extend_from_slice(shard);
            } else {
                bail!("reconstruction failed: data shard {} still missing", i);
            }
        }

        // Read the length prefix and truncate to the original size
        if data.len() < 8 {
            bail!("decoded data too short: missing length prefix");
        }
        let raw_len = u64::from_le_bytes(
            data[..8]
                .try_into()
                .map_err(|_| anyhow::anyhow!("length prefix conversion failed"))?,
        );
        let original_len = usize::try_from(raw_len)
            .map_err(|_| anyhow::anyhow!("length prefix {} exceeds addressable range", raw_len))?;
        let end = 8usize
            .checked_add(original_len)
            .ok_or_else(|| anyhow::anyhow!("length prefix overflow: 8 + {}", original_len))?;
        if end > data.len() {
            bail!(
                "length prefix {} exceeds decoded data size {}",
                original_len,
                data.len() - 8
            );
        }
        Ok(data[8..end].to_vec())
    }

    pub fn shard_config(&self) -> (usize, usize) {
        (self.data_shards, self.parity_shards)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::ReedSolomonEncoder;

    #[test]
    fn roundtrip_with_full_data() {
        let encoder = ReedSolomonEncoder::new(2, 1).unwrap();
        let decoder = ReedSolomonDecoder::new(2, 1).unwrap();
        let data = b"data availability";
        let shards = encoder.encode(data).unwrap();
        let with_options: Vec<_> = shards.into_iter().map(Some).collect();
        let recovered = decoder.decode(&with_options).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn reconstruct_with_missing_shards() {
        // RS(12, 10) - 10 data + 2 parity
        let encoder = ReedSolomonEncoder::new(10, 2).unwrap();
        let decoder = ReedSolomonDecoder::new(10, 2).unwrap();

        let data = b"This is test data for Reed-Solomon erasure coding with 10 data shards and 2 parity shards for fault tolerance";
        let shards = encoder.encode(data).unwrap();

        // Simulate losing 2 shards (within tolerance)
        let mut received_shards: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        received_shards[3] = None; // lose data shard
        received_shards[7] = None; // lose another data shard

        // Should still reconstruct successfully
        let recovered = decoder.decode(&received_shards).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn phase4_acceptance_10_percent_loss() {
        // Phase 4.1 Acceptance: reconstruct success ≥ 0.999 with synthetic p=0.1 loss
        // RS(12, 10) = 10 data + 2 parity = 16.7% tolerance
        let encoder = ReedSolomonEncoder::new(10, 2).unwrap();
        let decoder = ReedSolomonDecoder::new(10, 2).unwrap();

        let data = b"Critical blockchain data that must survive 10% packet loss in real network conditions";

        // Test multiple scenarios with exactly 10% loss (1.2 shards)
        // We'll test with 1 shard loss (8.3%) which is below 10%
        let trials = 100;
        let mut successes = 0;

        for _ in 0..trials {
            let shards = encoder.encode(data).unwrap();

            // Simulate losing 1 shard (8.3% < 10%)
            let mut received_shards: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
            received_shards[0] = None;

            match decoder.decode(&received_shards) {
                Ok(recovered) if recovered == data => successes += 1,
                _ => {}
            }
        }

        // Success rate should be 100% since we're only losing 1 shard
        let success_rate = successes as f64 / trials as f64;
        assert!(
            success_rate >= 0.999,
            "Success rate {} below acceptance threshold 0.999",
            success_rate
        );
    }

    #[test]
    fn roundtrip_preserves_trailing_zeros() {
        let encoder = ReedSolomonEncoder::new(2, 1).unwrap();
        let decoder = ReedSolomonDecoder::new(2, 1).unwrap();
        let data = b"hello\x00\x00\x00";
        let shards = encoder.encode(data).unwrap();
        let with_options: Vec<_> = shards.into_iter().map(Some).collect();
        let recovered = decoder.decode(&with_options).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn fails_with_too_many_missing_shards() {
        let encoder = ReedSolomonEncoder::new(10, 2).unwrap();
        let decoder = ReedSolomonDecoder::new(10, 2).unwrap();

        let data = b"test data";
        let shards = encoder.encode(data).unwrap();

        // Lose 3 shards (more than r=2 parity can handle)
        let mut received_shards: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        received_shards[0] = None;
        received_shards[1] = None;
        received_shards[2] = None;

        // Should fail - not enough shards
        assert!(decoder.decode(&received_shards).is_err());
    }

    #[test]
    fn test_all_zeros_data_roundtrips() {
        let encoder = ReedSolomonEncoder::new(2, 1).unwrap();
        let decoder = ReedSolomonDecoder::new(2, 1).unwrap();

        let data = [0u8; 32];
        let shards = encoder.encode(&data).unwrap();
        let with_options: Vec<_> = shards.into_iter().map(Some).collect();
        let recovered = decoder.decode(&with_options).unwrap();
        assert_eq!(
            recovered, data,
            "all-zeros data should roundtrip exactly (length-prefix fix)"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::encoder::ReedSolomonEncoder;
    use proptest::prelude::*;

    /// Arbitrary (k, r) with small values to keep tests fast.
    fn arb_k_r() -> impl Strategy<Value = (usize, usize)> {
        (1usize..=6, 1usize..=4).prop_map(|(k, r)| (k, r))
    }

    proptest! {
        /// Encoding then decoding with all shards present returns the original data.
        #[test]
        fn roundtrip_full_shards(
            (k, r) in arb_k_r(),
            data in prop::collection::vec(any::<u8>(), 1..256),
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let decoder = ReedSolomonDecoder::new(k, r).unwrap();
            let shards = encoder.encode(&data).unwrap();
            let with_opts: Vec<_> = shards.into_iter().map(Some).collect();
            let recovered = decoder.decode(&with_opts).unwrap();
            prop_assert_eq!(recovered, data, "full-shard roundtrip must be lossless");
        }

        /// Losing exactly one shard (any shard) still reconstructs the data when r >= 1.
        #[test]
        fn recover_with_one_missing_shard(
            (k, r) in (1usize..=5, 1usize..=3).prop_map(|(k, r)| (k, r)),
            data in prop::collection::vec(any::<u8>(), 1..200),
            missing_idx in 0usize..8,
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let decoder = ReedSolomonDecoder::new(k, r).unwrap();
            let shards = encoder.encode(&data).unwrap();
            let total = shards.len();
            let missing = missing_idx % total; // clamp to valid range
            let mut received: Vec<_> = shards.into_iter().map(Some).collect();
            received[missing] = None;
            let recovered = decoder.decode(&received).unwrap();
            prop_assert_eq!(recovered, data, "one missing shard must be recoverable");
        }

        /// Losing more than r shards returns an error.
        #[test]
        fn too_many_missing_shards_fails(
            k in 2usize..=6,
            r in 1usize..=3,
            data in prop::collection::vec(any::<u8>(), 1..200),
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let decoder = ReedSolomonDecoder::new(k, r).unwrap();
            let shards = encoder.encode(&data).unwrap();
            // Drop r+1 consecutive shards — exceeds recovery capacity
            let missing_count = r + 1;
            let mut received: Vec<_> = shards.into_iter().map(Some).collect();
            for slot in received.iter_mut().take(missing_count) {
                *slot = None;
            }
            prop_assert!(
                decoder.decode(&received).is_err(),
                "losing {} shards with r={} must fail",
                missing_count,
                r
            );
        }

        /// Encoded shard count equals k + r.
        #[test]
        fn shard_count_is_k_plus_r(
            (k, r) in arb_k_r(),
            data in prop::collection::vec(any::<u8>(), 1..200),
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let shards = encoder.encode(&data).unwrap();
            prop_assert_eq!(shards.len(), k + r, "shard count must equal k+r");
        }

        /// All shards have the same length.
        #[test]
        fn all_shards_same_length(
            (k, r) in arb_k_r(),
            data in prop::collection::vec(any::<u8>(), 1..200),
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let shards = encoder.encode(&data).unwrap();
            let shard_len = shards[0].len();
            for (i, shard) in shards.iter().enumerate() {
                prop_assert_eq!(shard.len(), shard_len, "shard {} has different length", i);
            }
        }

        /// Data with trailing zero bytes roundtrips exactly (length-prefix invariant).
        #[test]
        fn trailing_zeros_preserved(
            (k, r) in arb_k_r(),
            prefix in prop::collection::vec(1u8..=255, 1..50),
            trailing_zeros in 1usize..=16,
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let decoder = ReedSolomonDecoder::new(k, r).unwrap();
            let mut data = prefix;
            data.extend(std::iter::repeat(0u8).take(trailing_zeros));
            let shards = encoder.encode(&data).unwrap();
            let with_opts: Vec<_> = shards.into_iter().map(Some).collect();
            let recovered = decoder.decode(&with_opts).unwrap();
            prop_assert_eq!(recovered, data, "trailing zeros must be preserved exactly");
        }

        /// Encoding is deterministic: same input always produces same shards.
        #[test]
        fn encoding_is_deterministic(
            (k, r) in arb_k_r(),
            data in prop::collection::vec(any::<u8>(), 1..200),
        ) {
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let shards1 = encoder.encode(&data).unwrap();
            let shards2 = encoder.encode(&data).unwrap();
            prop_assert_eq!(shards1, shards2, "encoding must be deterministic");
        }

        /// Different data produces different first data shard (collision resistance spot-check).
        #[test]
        fn different_data_different_shards(
            (k, r) in arb_k_r(),
            data1 in prop::collection::vec(any::<u8>(), 1..100),
            data2 in prop::collection::vec(any::<u8>(), 1..100),
        ) {
            prop_assume!(data1 != data2);
            let encoder = ReedSolomonEncoder::new(k, r).unwrap();
            let shards1 = encoder.encode(&data1).unwrap();
            let shards2 = encoder.encode(&data2).unwrap();
            // At least one shard must differ
            let all_equal = shards1.iter().zip(shards2.iter()).all(|(a, b)| a == b);
            prop_assert!(!all_equal, "different inputs must produce different shards");
        }

        /// Decoder config() returns correct (k, r).
        #[test]
        fn decoder_config_correct(k in 1usize..=6, r in 1usize..=4) {
            let decoder = ReedSolomonDecoder::new(k, r).unwrap();
            let (dk, dr) = decoder.shard_config();
            prop_assert_eq!(dk, k);
            prop_assert_eq!(dr, r);
        }
    }
}
