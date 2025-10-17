use anyhow::{Context, Result};

/// Compresses data using zstd compression at level 3 (balanced speed/ratio).
/// Achieves ~10x+ compression ratio on typical blockchain state data.
pub fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::encode_all(bytes, 3).context("Failed to compress data with zstd")
}

/// Decompresses data that was compressed with zstd.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::decode_all(bytes).context("Failed to decompress data with zstd")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let original = b"Hello, World! This is test data that should compress well.";
        let compressed = compress(original).unwrap();
        let decompressed = decompress(&compressed).unwrap();

        assert_eq!(original.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn test_compression_ratio() {
        // Create repetitive data that should compress well (like blockchain state)
        let mut data = Vec::new();
        for _ in 0..1000 {
            data.extend_from_slice(b"account_balance_1000000000");
        }

        let original_size = data.len();
        let compressed = compress(&data).unwrap();
        let compressed_size = compressed.len();

        // Should achieve at least 5x compression on repetitive data
        let ratio = original_size as f64 / compressed_size as f64;
        assert!(
            ratio >= 5.0,
            "Compression ratio {:.2}x is less than 5x",
            ratio
        );
    }

    #[test]
    fn test_empty_data() {
        let empty: &[u8] = &[];
        let compressed = compress(empty).unwrap();
        let decompressed = decompress(&compressed).unwrap();

        assert_eq!(empty, decompressed.as_slice());
    }

    #[test]
    fn test_small_data() {
        let small = b"test";
        let compressed = compress(small).unwrap();
        let decompressed = decompress(&compressed).unwrap();

        assert_eq!(small.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn test_large_data() {
        // Create 1MB of data
        let large: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();

        let compressed = compress(&large).unwrap();
        let decompressed = decompress(&compressed).unwrap();

        assert_eq!(large, decompressed);

        // Verify some compression occurred
        let ratio = large.len() as f64 / compressed.len() as f64;
        assert!(ratio > 1.0, "Large data should compress");
    }

    #[test]
    fn test_invalid_compressed_data() {
        let invalid = b"this is not compressed data";
        let result = decompress(invalid);

        assert!(result.is_err(), "Should fail to decompress invalid data");
    }
}
