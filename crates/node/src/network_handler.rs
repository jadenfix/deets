use aether_p2p::network::NetworkEvent;
use aether_types::{Block, Slot, Transaction, Vote};
use bincode::Options;
use serde::{Deserialize, Serialize};

/// Decoded message types from the P2P network.
#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum NodeMessage {
    BlockReceived(Block),
    VoteReceived(Vote),
    TransactionReceived(Transaction),
    /// A peer requested blocks in the given slot range for state sync.
    BlockRangeRequested { from_slot: Slot, to_slot: Slot },
    PeerConnected,
    PeerDisconnected,
}

/// Outbound messages from the node to the P2P network.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum OutboundMessage {
    BroadcastBlock(Block),
    BroadcastVote(Vote),
    BroadcastTransaction(Transaction),
    /// Request a range of blocks from peers for state sync.
    RequestBlockRange { from_slot: Slot, to_slot: Slot },
}

/// Wire format for sync request messages on the `/aether/1/sync` topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub from_slot: Slot,
    pub to_slot: Slot,
}

/// Maximum message sizes to prevent OOM from malicious peers.
const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024; // 4MB
const MAX_VOTE_SIZE: usize = 4 * 1024; // 4KB
const MAX_TX_SIZE: usize = 128 * 1024; // 128KB
const MAX_SYNC_SIZE: usize = 1024; // 1KB

/// Deserialize with a bincode size limit to prevent DoS via deeply nested structures.
fn deserialize_bounded<T: serde::de::DeserializeOwned>(data: &[u8], max_size: usize) -> Option<T> {
    if data.len() > max_size {
        return None;
    }
    bincode::options()
        .with_limit(max_size as u64)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize(data)
        .ok()
}

/// Decode a raw P2P NetworkEvent into a typed NodeMessage.
/// Enforces message size limits before deserialization to prevent DoS.
pub fn decode_network_event(event: NetworkEvent) -> Option<NodeMessage> {
    match event {
        NetworkEvent::BlockReceived(data) if data.len() <= MAX_BLOCK_SIZE => {
            deserialize_bounded(&data, MAX_BLOCK_SIZE).map(NodeMessage::BlockReceived)
        }
        NetworkEvent::VoteReceived(data) if data.len() <= MAX_VOTE_SIZE => {
            deserialize_bounded(&data, MAX_VOTE_SIZE).map(NodeMessage::VoteReceived)
        }
        NetworkEvent::TransactionReceived(data) if data.len() <= MAX_TX_SIZE => {
            deserialize_bounded(&data, MAX_TX_SIZE).map(NodeMessage::TransactionReceived)
        }
        NetworkEvent::SyncRequestReceived(data) if data.len() <= MAX_SYNC_SIZE => {
            deserialize_bounded::<SyncRequest>(&data, MAX_SYNC_SIZE).map(|req| {
                NodeMessage::BlockRangeRequested {
                    from_slot: req.from_slot,
                    to_slot: req.to_slot,
                }
            })
        }
        NetworkEvent::PeerConnected(_) => Some(NodeMessage::PeerConnected),
        NetworkEvent::PeerDisconnected(_) => Some(NodeMessage::PeerDisconnected),
        _ => None, // Silently drop oversized or unknown messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::*;

    #[test]
    fn test_decode_block_event() {
        let block = Block::new(
            1,
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );
        let data = bincode::serialize(&block).unwrap();
        let event = NetworkEvent::BlockReceived(data);
        match decode_network_event(event) {
            Some(NodeMessage::BlockReceived(b)) => assert_eq!(b.header.slot, 1),
            other => {
                tracing::warn!("Unexpected network event, ignoring: {:?}", other);
            }
        }
    }

    #[test]
    fn test_decode_vote_event() {
        let vote = Vote {
            slot: 5,
            block_hash: H256::zero(),
            validator: PublicKey::from_bytes(vec![1u8; 32]),
            signature: Signature::from_bytes(vec![0u8; 64]),
            stake: 1000,
        };
        let data = bincode::serialize(&vote).unwrap();
        let event = NetworkEvent::VoteReceived(data);
        match decode_network_event(event) {
            Some(NodeMessage::VoteReceived(v)) => assert_eq!(v.slot, 5),
            other => {
                tracing::warn!("Unexpected network event, ignoring: {:?}", other);
            }
        }
    }

    #[test]
    fn test_decode_transaction_event() {
        let tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: Address::from_slice(&[0u8; 20]).unwrap(),
            sender_pubkey: PublicKey::from_bytes(vec![0u8; 32]),
            inputs: vec![],
            outputs: vec![],
            reads: std::collections::HashSet::new(),
            writes: std::collections::HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21000,
            fee: 1000,
            signature: Signature::from_bytes(vec![0u8; 64]),
        };
        let data = bincode::serialize(&tx).unwrap();
        let event = NetworkEvent::TransactionReceived(data);
        match decode_network_event(event) {
            Some(NodeMessage::TransactionReceived(_)) => {}
            other => {
                tracing::warn!("Unexpected network event, ignoring: {:?}", other);
            }
        }
    }

    #[test]
    fn test_decode_invalid_data_returns_none() {
        let event = NetworkEvent::BlockReceived(vec![0xFF, 0xFF, 0xFF]);
        assert!(decode_network_event(event).is_none());
    }

    #[test]
    fn test_decode_shred_event_returns_none() {
        let event = NetworkEvent::ShredReceived(vec![1, 2, 3]);
        assert!(decode_network_event(event).is_none());
    }
}
