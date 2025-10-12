// ============================================================================
// AETHER P2P - Peer-to-Peer Networking Layer
// ============================================================================
// PURPOSE: Node discovery, peer management, message propagation
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                      P2P NETWORK                                  │
// ├──────────────────────────────────────────────────────────────────┤
// │  Bootstrap Nodes  →  Peer Discovery (Kademlia DHT)               │
// │         ↓                              ↓                          │
// │  Peer Scoring  →  Connection Manager  →  Inbound/Outbound Limits │
// │         ↓                              ↓                          │
// │  Gossipsub Topics:                                                │
// │    - 'tx' (transactions)  →  Mempool                              │
// │    - 'header' (block proposals)  →  Consensus                     │
// │    - 'vote' (BLS votes)  →  Vote Aggregator                       │
// │    - 'shred' (erasure-coded blocks)  →  Turbine Reconstructor     │
// └──────────────────────────────────────────────────────────────────┘
//
// GOSSIPSUB TOPICS:
// - tx: Raw transaction propagation
// - header: Block header announcements
// - vote: Validator votes (BLS signatures)
// - shred: Erasure-coded block shards (Turbine)
//
// PEER SCORING:
// Score = base + stake_weight - penalties
// Penalties for: late messages, invalid data, rate limit violations
//
// PSEUDOCODE:
// ```
// struct P2PNetwork:
//     swarm: Libp2p::Swarm
//     peers: HashMap<PeerId, PeerInfo>
//     topics: HashMap<TopicName, Subscription>
//     config: P2PConfig
//
// fn start():
//     // Bootstrap
//     for bootstrap_addr in config.bootstrap_nodes:
//         dial(bootstrap_addr)
//
//     // Subscribe to topics
//     subscribe("tx")
//     subscribe("header")
//     subscribe("vote")
//     subscribe("shred")
//
//     // Event loop
//     loop:
//         match swarm.next_event():
//             PeerConnected(peer_id):
//                 handle_peer_connected(peer_id)
//             MessageReceived(topic, data):
//                 handle_message(topic, data)
//             PeerScoreUpdate(peer_id, score):
//                 if score < threshold:
//                     disconnect(peer_id)
//
// fn broadcast(topic, data):
//     gossipsub.publish(topic, data)
//
// fn send_direct(peer_id, data):
//     if peer = peers.get(peer_id):
//         peer.connection.send(data)
// ```
//
// ANTI-SPAM:
// - Rate limits per peer (tx/s, bytes/s)
// - Stake-weighted inbound quotas
// - Message deduplication (bloom filter)
//
// OUTPUTS:
// - Received messages → Node subsystems (Mempool, Consensus)
// - Peer list → Turbine routing
// - Network metrics → Monitoring
// ============================================================================

pub mod network;
pub mod peer_manager;
pub mod scoring;

pub use network::P2PNetwork;

