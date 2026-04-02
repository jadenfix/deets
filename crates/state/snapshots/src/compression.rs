use anyhow::Result;
use std::io::{Read, Write};

/// Compress data using zstd at level 3 (good speed/ratio balance).
pub fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(Vec::new(), 3)?;
    encoder.write_all(bytes)?;
    let compressed = encoder.finish()?;
    Ok(compressed)
}

/// Decompress zstd-compressed data.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zstd::Decoder::new(bytes)?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let data = b"hello world, this is a test of zstd compression!";
        let compressed = compress(data).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compression_reduces_size() {
        // Highly compressible data (repeated pattern)
        let data = vec![0xABu8; 10_000];
        let compressed = compress(&data).unwrap();
        assert!(
            compressed.len() < data.len() / 2,
            "compressed {} bytes to {} bytes",
            data.len(),
            compressed.len()
        );
    }

    #[test]
    fn test_empty_data() {
        let compressed = compress(b"").unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Compress-then-decompress is identity for arbitrary data.
        #[test]
        fn roundtrip(data in prop::collection::vec(any::<u8>(), 0..4096)) {
            let compressed = compress(&data).unwrap();
            let decompressed = decompress(&compressed).unwrap();
            prop_assert_eq!(decompressed, data);
        }

        /// Compression is deterministic — same input always produces same output.
        #[test]
        fn deterministic(data in prop::collection::vec(any::<u8>(), 0..2048)) {
            let a = compress(&data).unwrap();
            let b = compress(&data).unwrap();
            prop_assert_eq!(a, b);
        }

        /// Decompressing corrupted data returns an error, not garbage.
        #[test]
        fn corrupted_data_errors(data in prop::collection::vec(any::<u8>(), 1..512)) {
            let compressed = compress(&data).unwrap();
            if compressed.len() > 4 {
                let mut corrupted = compressed.clone();
                // Flip a byte in the middle of the compressed payload
                let mid = corrupted.len() / 2;
                corrupted[mid] ^= 0xFF;
                // Should either error or produce different output
                match decompress(&corrupted) {
                    Err(_) => {} // expected
                    Ok(out) => prop_assert_ne!(out, data, "corrupted data should not roundtrip cleanly"),
                }
            }
        }
    }
}
