use aether_types::{Block, BlockHeader, Transaction, H256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Compact block representation for bandwidth-efficient propagation.
///
/// Instead of sending full blocks (which can be 2MB with 5000 txs),
/// sends only the header + transaction hashes. Peers reconstruct
/// the full block from their mempool.
///
/// Bandwidth savings: ~90% for validators with warm mempools.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompactBlock {
    pub header: BlockHeader,
    pub tx_hashes: Vec<H256>,
}

/// Result of attempting to reconstruct a full block from a compact block.
pub struct ReconstructionResult {
    /// The reconstructed block (if all txs were available).
    pub block: Option<Block>,
    /// Transaction hashes that are missing from the local mempool.
    pub missing: Vec<H256>,
}

impl CompactBlock {
    /// Create a compact block from a full block.
    pub fn from_block(block: &Block) -> Self {
        CompactBlock {
            header: block.header.clone(),
            tx_hashes: block.transactions.iter().map(|tx| tx.hash()).collect(),
        }
    }

    /// Attempt to reconstruct the full block using locally available transactions.
    pub fn reconstruct(&self, known_txs: &HashMap<H256, Transaction>) -> ReconstructionResult {
        let mut transactions = Vec::with_capacity(self.tx_hashes.len());
        let mut missing = Vec::new();

        for hash in &self.tx_hashes {
            match known_txs.get(hash) {
                Some(tx) => transactions.push(tx.clone()),
                None => missing.push(*hash),
            }
        }

        if missing.is_empty() {
            ReconstructionResult {
                block: Some(Block {
                    header: self.header.clone(),
                    transactions,
                    aggregated_vote: None,
                }),
                missing: vec![],
            }
        } else {
            ReconstructionResult {
                block: None,
                missing,
            }
        }
    }

    /// Serialized size of this compact block.
    pub fn wire_size(&self) -> usize {
        bincode::serialize(self).map(|b| b.len()).unwrap_or(0)
    }
}

/// Compress data with zstd for P2P messages > 256 bytes.
pub fn compress_message(data: &[u8]) -> Vec<u8> {
    if data.len() <= 256 {
        // Prepend 0x00 = uncompressed
        let mut out = Vec::with_capacity(1 + data.len());
        out.push(0x00);
        out.extend_from_slice(data);
        return out;
    }

    // Prepend 0x01 = zstd compressed
    match zstd::encode_all(data, 1) {
        Ok(compressed) => {
            let mut out = Vec::with_capacity(1 + compressed.len());
            out.push(0x01);
            out.extend_from_slice(&compressed);
            out
        }
        Err(_) => {
            // Fallback to uncompressed
            let mut out = Vec::with_capacity(1 + data.len());
            out.push(0x00);
            out.extend_from_slice(data);
            out
        }
    }
}

/// Decompress a P2P message.
pub fn decompress_message(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.is_empty() {
        anyhow::bail!("empty message");
    }

    match data[0] {
        0x00 => Ok(data[1..].to_vec()),
        0x01 => {
            let decompressed = zstd::decode_all(&data[1..])?;
            Ok(decompressed)
        }
        tag => anyhow::bail!("unknown compression tag: {}", tag),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::*;

    fn make_test_block(num_txs: usize) -> Block {
        let txs: Vec<Transaction> = (0..num_txs)
            .map(|i| Transaction {
                nonce: i as u64,
                sender: Address::from_slice(&[1u8; 20]).unwrap(),
                sender_pubkey: PublicKey::from_bytes(vec![2u8; 32]),
                inputs: vec![],
                outputs: vec![],
                reads: std::collections::HashSet::new(),
                writes: std::collections::HashSet::new(),
                program_id: None,
                data: vec![0u8; 100], // 100 bytes of tx data each
                gas_limit: 21000,
                fee: 1000,
                signature: Signature::from_bytes(vec![3u8; 64]),
            })
            .collect();

        Block::new(
            0,
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            VrfProof {
                output: [0u8; 32],
                proof: vec![0u8; 80],
            },
            txs,
        )
    }

    #[test]
    fn test_compact_block_smaller_than_full() {
        let block = make_test_block(100);
        let full_size = bincode::serialize(&block).unwrap().len();

        let compact = CompactBlock::from_block(&block);
        let compact_size = compact.wire_size();

        println!(
            "Full block: {} bytes, Compact: {} bytes, Savings: {:.0}%",
            full_size,
            compact_size,
            (1.0 - compact_size as f64 / full_size as f64) * 100.0
        );

        assert!(
            compact_size < full_size / 2,
            "compact block should be at least 50% smaller"
        );
    }

    #[test]
    fn test_reconstruct_with_all_txs() {
        let block = make_test_block(10);
        let compact = CompactBlock::from_block(&block);

        // Build known txs map
        let known: HashMap<H256, Transaction> = block
            .transactions
            .iter()
            .map(|tx| (tx.hash(), tx.clone()))
            .collect();

        let result = compact.reconstruct(&known);
        assert!(result.block.is_some());
        assert!(result.missing.is_empty());
        assert_eq!(result.block.unwrap().transactions.len(), 10);
    }

    #[test]
    fn test_reconstruct_with_missing_txs() {
        let block = make_test_block(10);
        let compact = CompactBlock::from_block(&block);

        // Only have first 5 txs
        let known: HashMap<H256, Transaction> = block.transactions[..5]
            .iter()
            .map(|tx| (tx.hash(), tx.clone()))
            .collect();

        let result = compact.reconstruct(&known);
        assert!(result.block.is_none());
        assert_eq!(result.missing.len(), 5);
    }

    #[test]
    fn test_compression_roundtrip() {
        let data = vec![42u8; 1000]; // 1KB repeated data
        let compressed = compress_message(&data);
        let decompressed = decompress_message(&compressed).unwrap();
        assert_eq!(decompressed, data);
        assert!(compressed.len() < data.len() / 2, "should compress well");
    }

    #[test]
    fn test_small_message_not_compressed() {
        let data = vec![42u8; 100]; // Small message
        let compressed = compress_message(&data);
        assert_eq!(compressed[0], 0x00, "small messages should not be compressed");
        let decompressed = decompress_message(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }
}
