use aether_p2p::network::NetworkEvent;
use aether_types::{Block, Transaction, Vote};
use bincode::Options;

/// Decoded message types from the P2P network.
#[derive(Debug)]
pub enum NodeMessage {
    BlockReceived(Block),
    VoteReceived(Vote),
    TransactionReceived(Transaction),
}

/// Outbound messages from the node to the P2P network.
#[derive(Debug, Clone)]
pub enum OutboundMessage {
    BroadcastBlock(Block),
    BroadcastVote(Vote),
    BroadcastTransaction(Transaction),
}

/// Maximum message sizes to prevent OOM from malicious peers.
const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024; // 4MB
const MAX_VOTE_SIZE: usize = 4 * 1024; // 4KB
const MAX_TX_SIZE: usize = 128 * 1024; // 128KB

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
            deserialize_bounded(&data, MAX_BLOCK_SIZE)
                .map(NodeMessage::BlockReceived)
        }
        NetworkEvent::VoteReceived(data) if data.len() <= MAX_VOTE_SIZE => {
            deserialize_bounded(&data, MAX_VOTE_SIZE)
                .map(NodeMessage::VoteReceived)
        }
        NetworkEvent::TransactionReceived(data) if data.len() <= MAX_TX_SIZE => {
            deserialize_bounded(&data, MAX_TX_SIZE)
                .map(NodeMessage::TransactionReceived)
        }
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
            other => panic!("expected BlockReceived, got {:?}", other),
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
            other => panic!("expected VoteReceived, got {:?}", other),
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
            other => panic!("expected TransactionReceived, got {:?}", other),
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
