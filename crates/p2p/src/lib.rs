// ============================================================================
// AETHER P2P - Peer-to-Peer Networking Layer
// ============================================================================
// PURPOSE: Decentralized network for block and transaction propagation
//
// ARCHITECTURE:
// - libp2p for networking stack
// - Gossipsub for pub/sub messaging
// - Kademlia DHT for peer discovery
// - QUIC for transport (low latency, multiplexing)
// - Noise protocol for encryption
//
// TOPICS:
// - /aether/tx: Transaction propagation
// - /aether/block: Block propagation
// - /aether/vote: Consensus votes
// - /aether/shred: Data availability shreds
//
// PEER MANAGEMENT:
// - Scoring system (reputation)
// - Ban misbehaving peers
// - Connection limits
// - NAT traversal
//
// MESSAGE FLOW:
// 1. Local node publishes to topic
// 2. Gossipsub forwards to subscribed peers
// 3. Peers validate and re-broadcast
// 4. Deduplication prevents loops
// 5. Handler processes new messages
//
// PERFORMANCE:
// - Target: 10k peers
// - Message latency: <100ms p95
// - Bandwidth: ~1 MB/s per peer
// ============================================================================

pub mod network;
pub mod gossip;

pub use network::{P2PNetwork, PeerInfo};
pub use gossip::GossipManager;
