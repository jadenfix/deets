use anyhow::{bail, Result};

use crate::encoder::ReedSolomonEncoder;

#[derive(Debug, Clone, Copy)]
pub struct ReedSolomonDecoder {
    encoder: ReedSolomonEncoder,
}

impl ReedSolomonDecoder {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self> {
        Ok(ReedSolomonDecoder {
            encoder: ReedSolomonEncoder::new(data_shards, parity_shards)?,
        })
    }

    pub fn decode(&self, shards: &[Option<Vec<u8>>]) -> Result<Vec<u8>> {
        let expected = self.encoder.data_shards + self.encoder.parity_shards;
        if shards.len() != expected {
            bail!(
                "expected {} shards (data + parity), got {}",
                expected,
                shards.len()
            );
        }

        let mut data = Vec::new();
        let mut missing = Vec::new();

        for (idx, shard) in shards.iter().enumerate().take(self.encoder.data_shards) {
            match shard {
                Some(bytes) => data.extend_from_slice(bytes),
                None => missing.push(idx),
            }
        }

        if !missing.is_empty() {
            bail!("missing data shards: {:?}", missing);
        }

        while data.last() == Some(&0u8) {
            data.pop();
        }

        Ok(data)
    }

    pub fn shard_config(&self) -> (usize, usize) {
        (self.encoder.data_shards, self.encoder.parity_shards)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
