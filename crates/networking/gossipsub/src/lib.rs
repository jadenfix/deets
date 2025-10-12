// ============================================================================
// AETHER GOSSIPSUB - Publish-Subscribe Message Propagation
// ============================================================================
// PURPOSE: Efficient broadcast of transactions, blocks, votes across network
//
// PROTOCOL: Gossipsub (epidemic broadcast with mesh overlay)
//
// TOPICS:
// - tx: New transactions (user submissions)
// - header: Block proposals (leaders)
// - vote: Validator votes (consensus)
// - shred: Erasure-coded block shards (Turbine)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    GOSSIPSUB LAYER                                │
// ├──────────────────────────────────────────────────────────────────┤
// │  Local Publish  →  Topic Mesh  →  Forward to D peers             │
// │         ↓                              ↓                          │
// │  Receive from Peer  →  Deduplicate  →  Validate  →  Forward      │
// │         ↓                              ↓                          │
// │  Deliver to Subscriber  →  Mempool/Consensus/Turbine             │
// └──────────────────────────────────────────────────────────────────┘
//
// MESH MAINTENANCE:
// - D peers in mesh per topic (target: 8)
// - Periodic GRAFT/PRUNE messages
// - Peer scoring (deliver quickly, valid messages)
//
// PSEUDOCODE:
// ```
// struct Gossipsub:
//     mesh: HashMap<Topic, HashSet<PeerId>>
//     subscriptions: HashMap<Topic, Vec<Subscriber>>
//     seen_messages: BloomFilter
//
// fn publish(topic, data):
//     msg = Message { topic, data, id: hash(data) }
//     
//     if seen_messages.contains(msg.id):
//         return
//     
//     seen_messages.insert(msg.id)
//     
//     // Forward to mesh peers
//     for peer in mesh[topic]:
//         send_to_peer(peer, msg)
//     
//     // Deliver locally
//     for subscriber in subscriptions[topic]:
//         subscriber.handle(data)
//
// fn handle_received(msg):
//     if seen_messages.contains(msg.id):
//         return  // Already seen
//     
//     seen_messages.insert(msg.id)
//     
//     if validate_message(msg):
//         // Forward to mesh (except sender)
//         forward_to_mesh(msg)
//         
//         // Deliver locally
//         deliver_to_subscribers(msg)
//     else:
//         penalize_sender(msg.sender)
// ```
//
// OUTPUTS:
// - Delivered messages → Subscribers (Mempool, Consensus)
// - Peer scores → Connection manager
// - Propagation metrics → Monitoring
// ============================================================================

pub mod router;
pub mod mesh;
pub mod scoring;

pub use router::GossipRouter;

