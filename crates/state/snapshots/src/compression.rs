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
