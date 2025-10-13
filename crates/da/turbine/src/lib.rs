// ============================================================================
// AETHER TURBINE - Block Propagation via Tree Sharding
// ============================================================================
// PURPOSE: Distribute large blocks quickly without bottlenecking leader
//
// ALGORITHM: Turbine (Solana-style sharded fan-out with erasure coding)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    TURBINE DATA AVAILABILITY                      │
// ├──────────────────────────────────────────────────────────────────┤
// │  Block (2MB)  →  Reed-Solomon Encode  →  Shreds (RS(12,10))      │
// │         ↓                                      ↓                  │
// │  Tree Topology (stake-weighted)  →  Route Shreds to Subtrees     │
// │         ↓                                      ↓                  │
// │  Each Peer Receives k/N shreds  →  Retransmit to Children        │
// │         ↓                                      ↓                  │
// │  Reconstruct with k shreds  →  Validate  →  Execute Block        │
// └──────────────────────────────────────────────────────────────────┘
//
// SHARDING:
// Block → Encode with RS(n, k) → n shreds (any k sufficient to reconstruct)
//
// Example: RS(12, 10)
//   - 10 data shards
//   - 2 parity shards
//   - Any 10 of 12 shreds can reconstruct block
//   - Tolerates 2 lost shreds (16% loss)
//
// TREE ROUTING:
// ```
// Leader (root) has block
//   → Encode to 12 shreds
//   → Send shred_i to validator_i in layer 1
//
// Each layer-1 validator:
//   → Receives 1 shred
//   → Retransmits to children in layer 2
//
// Result: O(log N) latency, O(1) bandwidth per node
// ```
//
// PSEUDOCODE:
// ```
// struct Turbine:
//     topology: StakeWeightedTree
//     shreds: HashMap<BlockHash, Vec<Shred>>
//     reconstructor: Reconstructor
//
// fn broadcast_as_leader(block):
//     shreds = erasure_code(block, k=10, r=2)
//
//     // Send to layer-1 validators
//     layer1 = topology.get_layer(1)
//     for (i, validator) in enumerate(layer1):
//         send_shred(validator, shreds[i])
//
// fn handle_received_shred(shred):
//     block_id = shred.block_hash
//     shreds[block_id].push(shred)
//
//     // Retransmit to children
//     children = topology.get_children(my_id)
//     for child in children:
//         send_shred(child, shred)
//
//     // Try to reconstruct
//     if shreds[block_id].len() >= k:
//         block = erasure_decode(shreds[block_id])
//         if block:
//             deliver_block(block)
//
// fn erasure_code(block, k, r) -> Vec<Shred>:
//     chunks = split(block, k)
//     encoder = ReedSolomon::new(k, r)
//     parity = encoder.encode(chunks)
//     return chunks + parity
//
// fn erasure_decode(shreds) -> Option<Block>:
//     if shreds.len() < k:
//         return None
//
//     decoder = ReedSolomon::new(k, r)
//     data_chunks = decoder.decode(shreds[0..k])
//     return join(data_chunks)
// ```
//
// TOPOLOGY:
// - Stake-weighted tree construction
// - Higher stake = higher in tree (lower latency)
// - Periodically rebuild (per epoch)
//
// PERFORMANCE:
// - 2MB block, 12 shreds = ~170KB per shred
// - 500ms slot → need <200ms propagation
// - Tree depth 3 → 3 hops × 50ms RTT = 150ms ✓
//
// OUTPUTS:
// - Reconstructed blocks → Consensus
// - Missing shred requests → Repair protocol
// - Propagation metrics → Monitoring
// ============================================================================

pub mod broadcast;
pub mod receive;
pub mod repair;
pub mod topology;

pub use broadcast::TurbineBroadcaster;
pub use receive::TurbineReceiver;

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::H256;
    use rand::{seq::SliceRandom, Rng};
    use sha2::{Digest, Sha256};

    #[test]
    fn phase4_acceptance_turbine_packet_loss_resilience() {
        const DATA_SHARDS: usize = 10;
        const PARITY_SHARDS: usize = 2;
        const TOTAL_SHREDS: usize = DATA_SHARDS + PARITY_SHARDS;
        const TRIALS: usize = 200;

        let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
        let mut rng = rand::thread_rng();
        let mut successes = 0usize;

        for trial in 0..TRIALS {
            let payload = format!("phase4 turbine payload {trial}").into_bytes();
            let block_hash = {
                let digest = Sha256::digest(&payload);
                H256::from_slice(&digest).unwrap()
            };

            let shreds = broadcaster
                .make_shreds(1, block_hash, &payload)
                .expect("shards");

            // Randomly drop up to parity shards to simulate <=16% loss
            let drop_count = rng.gen_range(0..=PARITY_SHARDS);
            let mut indices: Vec<usize> = (0..TOTAL_SHREDS).collect();
            indices.shuffle(&mut rng);
            let drop_set: std::collections::HashSet<_> =
                indices.into_iter().take(drop_count).collect();

            let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
            let mut recovered = false;

            for (idx, shred) in shreds.into_iter().enumerate() {
                if drop_set.contains(&idx) {
                    continue;
                }
                if let Some(block) = receiver.ingest_shred(shred).unwrap() {
                    assert_eq!(block, payload);
                    recovered = true;
                    successes += 1;
                    break;
                }
            }

            assert!(
                recovered,
                "failed to reconstruct block despite <= parity loss (trial {trial})"
            );
        }

        let success_rate = successes as f64 / TRIALS as f64;
        assert!(
            success_rate >= 0.999,
            "success rate {} below acceptance threshold",
            success_rate
        );
    }
}
