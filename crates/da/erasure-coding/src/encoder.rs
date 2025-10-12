use anyhow::{bail, Result};

/// Simple Reed-Solomon style encoder that splits input into data shards and
/// appends parity shards by XOR'ing the data shards together. This keeps the
/// interface close to the production design while remaining lightweight for
/// tests.
#[derive(Debug, Clone, Copy)]
pub struct ReedSolomonEncoder {
    pub data_shards: usize,
    pub parity_shards: usize,
}

impl ReedSolomonEncoder {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        if data_shards == 0 {
            bail!("data shards must be non-zero");
        }
        if parity_shards == 0 {
            bail!("parity shards must be non-zero");
        }

        Ok(ReedSolomonEncoder {
            data_shards,
            parity_shards,
        })
    }

    #[allow(clippy::manual_div_ceil)]
    pub fn shard_size(&self, data_len: usize) -> usize {
        (data_len + self.data_shards - 1) / self.data_shards
    }

    pub fn encode(&self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let chunk_size = self.shard_size(data.len());
        if chunk_size == 0 {
            return Ok(vec![vec![]; self.data_shards + self.parity_shards]);
        }

        let mut shards = Vec::with_capacity(self.data_shards + self.parity_shards);

        for shard_index in 0..self.data_shards {
            let start = shard_index * chunk_size;
            let end = (start + chunk_size).min(data.len());
            let mut chunk = vec![0u8; chunk_size];

            if start < data.len() {
                chunk[..end - start].copy_from_slice(&data[start..end]);
            }

            shards.push(chunk);
        }

        let parity_chunks = self.build_parity_chunks(&shards);
        shards.extend(parity_chunks);

        Ok(shards)
    }

    fn build_parity_chunks(&self, data_shards: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let chunk_size = data_shards.first().map(|s| s.len()).unwrap_or(0);
        let mut parity_chunks = Vec::with_capacity(self.parity_shards);

        for parity_idx in 0..self.parity_shards {
            let mut chunk = vec![0u8; chunk_size];
            for (data_idx, data_chunk) in data_shards.iter().enumerate() {
                for (byte_idx, byte) in data_chunk.iter().enumerate() {
                    let tweak = (parity_idx as u8).wrapping_add(data_idx as u8);
                    chunk[byte_idx] ^= byte.wrapping_add(tweak);
                }
            }
            parity_chunks.push(chunk);
        }

        parity_chunks
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
