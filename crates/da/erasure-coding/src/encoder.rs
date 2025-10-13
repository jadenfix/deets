use anyhow::{bail, Result};
use reed_solomon_erasure::galois_8::ReedSolomon;

/// Production Reed-Solomon encoder using Cauchy matrix method
/// for erasure coding. This implementation provides proper mathematical
/// Reed-Solomon encoding with optimal recovery properties.
///
/// Parameters: RS(n, k) where n = k + r
/// - k: number of data shards
/// - r: number of parity shards
/// - Any k shards can recover original data
///
/// Example: RS(12, 10) = 10 data + 2 parity
/// - Can tolerate loss of any 2 shards
/// - 16.7% packet loss tolerance
#[derive(Debug)]
pub struct ReedSolomonEncoder {
    pub data_shards: usize,
    pub parity_shards: usize,
    encoder: ReedSolomon,
}

impl ReedSolomonEncoder {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        if data_shards == 0 {
            bail!("data shards must be non-zero");
        }
        if parity_shards == 0 {
            bail!("parity shards must be non-zero");
        }

        let encoder = ReedSolomon::new(data_shards, parity_shards)
            .map_err(|e| anyhow::anyhow!("failed to create RS encoder: {}", e))?;

        Ok(ReedSolomonEncoder {
            data_shards,
            parity_shards,
            encoder,
        })
    }

    #[allow(clippy::manual_div_ceil)]
    pub fn shard_size(&self, data_len: usize) -> usize {
        (data_len + self.data_shards - 1) / self.data_shards
    }

    /// Encode data into data+parity shards using Reed-Solomon erasure coding
    pub fn encode(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let chunk_size = self.shard_size(data.len());
        if chunk_size == 0 {
            return Ok(vec![vec![]; self.data_shards + self.parity_shards]);
        }

        // Split data into equal-sized shards
        let mut shards = Vec::with_capacity(self.data_shards + self.parity_shards);

        // Create data shards
        for shard_index in 0..self.data_shards {
            let start = shard_index * chunk_size;
            let end = (start + chunk_size).min(data.len());
            let mut chunk = vec![0u8; chunk_size];

            if start < data.len() {
                chunk[..end - start].copy_from_slice(&data[start..end]);
            }

            shards.push(chunk);
        }

        // Add empty parity shards
        for _ in 0..self.parity_shards {
            shards.push(vec![0u8; chunk_size]);
        }

        // Encode in-place
        self.encoder
            .encode(&mut shards)
            .map_err(|e| anyhow::anyhow!("encoding failed: {}", e))?;

        Ok(shards)
    }

    /// Total number of shards (data + parity)
    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_into_expected_shards() {
        let encoder = ReedSolomonEncoder::new(3, 2).unwrap();
        let data = b"hello world";
        let shards = encoder.encode(data).unwrap();
        assert_eq!(shards.len(), 5);
        assert_eq!(shards[0].len(), encoder.shard_size(data.len()));
    }
}
