use aether_types::{Block, Transaction};
use anyhow::Result;
use libp2p::futures::StreamExt;
use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify,
    identity::Keypair,
    kad,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

/// Topics for Aether network gossip.
pub const TOPIC_TX: &str = "/aether/1/tx";
pub const TOPIC_BLOCK: &str = "/aether/1/block";
pub const TOPIC_VOTE: &str = "/aether/1/vote";
pub const TOPIC_SHRED: &str = "/aether/1/shred";

/// Events emitted by the P2P network to the node.
#[derive(Debug)]
pub enum NetworkEvent {
    TransactionReceived(Vec<u8>),
    BlockReceived(Vec<u8>),
    VoteReceived(Vec<u8>),
    ShredReceived(Vec<u8>),
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
}

/// Composite libp2p behaviour for Aether.
#[derive(NetworkBehaviour)]
struct AetherBehaviour {
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    identify: identify::Behaviour,
}

/// Production P2P network using libp2p.
/// Ban duration for misbehaving peers (1 hour).
const BAN_DURATION_SECS: u64 = 3600;

pub struct P2PNetwork {
    swarm: Swarm<AetherBehaviour>,
    local_peer_id: PeerId,
    topics: HashMap<String, IdentTopic>,
    event_tx: mpsc::Sender<NetworkEvent>,
    event_rx: mpsc::Receiver<NetworkEvent>,
    peers: HashMap<PeerId, PeerInfo>,
    /// Banned peers with expiry timestamps. Peers cannot reconnect until ban expires.
    banned_peers: HashMap<PeerId, u64>,
}

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub id: String,
    pub address: String,
    pub score: i32,
    pub connected_at: u64,
}

impl P2PNetwork {
    /// Create a new P2P network with a random keypair.
    pub fn new(keypair: Keypair) -> Result<Self> {
        let local_peer_id = PeerId::from(keypair.public());

        // Configure gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(ValidationMode::Strict)
            .mesh_n(8)
            .mesh_n_low(4)
            .mesh_n_high(12)
            .gossip_lazy(6)
            .history_length(5)
            .history_gossip(3)
            .max_transmit_size(2 * 1024 * 1024) // 2MB blocks
            .build()
            .map_err(|e| anyhow::anyhow!("gossipsub config error: {}", e))?;

        let gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow::anyhow!("gossipsub init error: {}", e))?;

        // Configure Kademlia
        let store = kad::store::MemoryStore::new(local_peer_id);
        let kademlia = kad::Behaviour::new(local_peer_id, store);

        // Configure Identify
        let identify = identify::Behaviour::new(identify::Config::new(
            "/aether/1.0.0".to_string(),
            keypair.public(),
        ));

        let behaviour = AetherBehaviour {
            gossipsub,
            kademlia,
            identify,
        };

        let swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|_| Ok(behaviour))
            .map_err(|e| anyhow::anyhow!("swarm build error: {}", e))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let (event_tx, event_rx) = mpsc::channel(1024);

        Ok(P2PNetwork {
            swarm,
            local_peer_id,
            topics: HashMap::new(),
            event_tx,
            event_rx,
            peers: HashMap::new(),
            banned_peers: HashMap::new(),
        })
    }

    /// Create with a random keypair (convenience).
    pub fn new_random() -> Result<Self> {
        let keypair = Keypair::generate_ed25519();
        Self::new(keypair)
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    pub fn peer_id_str(&self) -> String {
        self.local_peer_id.to_string()
    }

    /// Start listening on the given address.
    pub async fn start(&mut self, listen_addr: &str) -> Result<()> {
        let addr: Multiaddr = listen_addr
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid listen address: {}", e))?;
        self.swarm.listen_on(addr)?;

        // Subscribe to all standard topics
        self.subscribe(TOPIC_TX)?;
        self.subscribe(TOPIC_BLOCK)?;
        self.subscribe(TOPIC_VOTE)?;
        self.subscribe(TOPIC_SHRED)?;

        Ok(())
    }

    /// Subscribe to a gossipsub topic.
    pub fn subscribe(&mut self, topic_str: &str) -> Result<()> {
        if self.topics.contains_key(topic_str) {
            return Ok(());
        }
        let topic = IdentTopic::new(topic_str);
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&topic)
            .map_err(|e| anyhow::anyhow!("subscribe error: {}", e))?;
        self.topics.insert(topic_str.to_string(), topic);
        Ok(())
    }

    /// Publish data to a gossipsub topic.
    pub fn publish(&mut self, topic_str: &str, data: Vec<u8>) -> Result<()> {
        let topic = self
            .topics
            .get(topic_str)
            .ok_or_else(|| anyhow::anyhow!("not subscribed to topic: {}", topic_str))?;
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic.clone(), data)
            .map_err(|e| anyhow::anyhow!("publish error: {}", e))?;
        Ok(())
    }

    /// Broadcast a transaction.
    pub fn broadcast_transaction(&mut self, tx: &Transaction) -> Result<()> {
        let data = bincode::serialize(tx)?;
        self.publish(TOPIC_TX, data)
    }

    /// Broadcast a block.
    pub fn broadcast_block(&mut self, block: &Block) -> Result<()> {
        let data = bincode::serialize(block)?;
        self.publish(TOPIC_BLOCK, data)
    }

    /// Connect to a peer by multiaddr.
    pub fn connect_peer(&mut self, addr: &str) -> Result<()> {
        let multiaddr: Multiaddr = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid multiaddr: {}", e))?;
        self.swarm.dial(multiaddr)?;
        Ok(())
    }

    /// Add a known bootstrap peer for Kademlia.
    pub fn add_bootstrap_peer(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.swarm
            .behaviour_mut()
            .kademlia
            .add_address(&peer_id, addr);
    }

    /// Get connected peer count.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get connected peers.
    pub fn get_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Take the event receiver (for the node to consume events).
    pub fn take_event_rx(&mut self) -> mpsc::Receiver<NetworkEvent> {
        let (new_tx, new_rx) = mpsc::channel(1024);
        let old_rx = std::mem::replace(&mut self.event_rx, new_rx);
        self.event_tx = new_tx;
        old_rx
    }

    /// Poll the swarm for events. Call this in a loop from the node.
    pub async fn poll(&mut self) -> Option<NetworkEvent> {
        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(AetherBehaviourEvent::Gossipsub(
                    gossipsub::Event::Message { message, .. },
                )) => {
                    let topic = message.topic.to_string();
                    let data = message.data;

                    let event = if topic.contains("/tx") {
                        NetworkEvent::TransactionReceived(data)
                    } else if topic.contains("/block") {
                        NetworkEvent::BlockReceived(data)
                    } else if topic.contains("/vote") {
                        NetworkEvent::VoteReceived(data)
                    } else if topic.contains("/shred") {
                        NetworkEvent::ShredReceived(data)
                    } else {
                        continue;
                    };

                    return Some(event);
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    // Check if peer is banned
                    if let Some(&ban_expiry) = self.banned_peers.get(&peer_id) {
                        if current_timestamp() < ban_expiry {
                            // Still banned — disconnect immediately
                            let _ = self.swarm.disconnect_peer_id(peer_id);
                            continue;
                        } else {
                            // Ban expired — remove from list
                            self.banned_peers.remove(&peer_id);
                        }
                    }

                    let info = PeerInfo {
                        id: peer_id.to_string(),
                        address: String::new(),
                        score: 0,
                        connected_at: current_timestamp(),
                    };
                    self.peers.insert(peer_id, info);
                    return Some(NetworkEvent::PeerConnected(peer_id));
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    self.peers.remove(&peer_id);
                    return Some(NetworkEvent::PeerDisconnected(peer_id));
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!("Listening on {}/p2p/{}", address, self.local_peer_id);
                    continue;
                }
                _ => continue,
            }
        }
    }

    /// Update a peer's reputation score.
    pub fn update_peer_score(&mut self, peer_id: &PeerId, delta: i32) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.score += delta;
            if info.score < -100 {
                // Ban for BAN_DURATION_SECS, disconnect
                let ban_expiry = current_timestamp() + BAN_DURATION_SECS;
                self.banned_peers.insert(*peer_id, ban_expiry);
                let _ = self.swarm.disconnect_peer_id(*peer_id);
                self.peers.remove(peer_id);
            }
        }
    }

    /// Check if a peer is currently banned.
    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.banned_peers
            .get(peer_id)
            .map_or(false, |&expiry| current_timestamp() < expiry)
    }

    /// Get count of currently banned peers.
    pub fn banned_count(&self) -> usize {
        let now = current_timestamp();
        self.banned_peers.values().filter(|&&expiry| now < expiry).count()
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_creation() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let network = P2PNetwork::new_random().unwrap();
            assert_eq!(network.peer_count(), 0);
            assert!(!network.peer_id_str().is_empty());
        });
    }

    #[test]
    fn test_subscribe_topics() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut network = P2PNetwork::new_random().unwrap();
            network.subscribe(TOPIC_TX).unwrap();
            network.subscribe(TOPIC_BLOCK).unwrap();
            assert_eq!(network.topics.len(), 2);
        });
    }

    #[tokio::test]
    async fn test_two_nodes_connect() {
        let mut node1 = P2PNetwork::new_random().unwrap();
        let mut node2 = P2PNetwork::new_random().unwrap();

        node1.start("/ip4/127.0.0.1/tcp/0").await.unwrap();
        node2.start("/ip4/127.0.0.1/tcp/0").await.unwrap();

        // Poll node1 to process the NewListenAddr event
        let mut node1_addr = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(200), node1.swarm.select_next_some()).await {
                Ok(SwarmEvent::NewListenAddr { address, .. }) => {
                    node1_addr = Some(address);
                    break;
                }
                _ => continue,
            }
        }

        let node1_addr = node1_addr.expect("node1 should have a listen address");
        let dial_addr = format!("{}/p2p/{}", node1_addr, node1.local_peer_id);
        node2.connect_peer(&dial_addr).unwrap();

        // Poll both nodes to process connection events
        let mut connected = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && !connected {
            tokio::select! {
                event = node1.swarm.select_next_some() => {
                    if matches!(event, SwarmEvent::ConnectionEstablished { .. }) {
                        connected = true;
                    }
                }
                event = node2.swarm.select_next_some() => {
                    if matches!(event, SwarmEvent::ConnectionEstablished { .. }) {
                        connected = true;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }

        assert!(connected, "nodes should connect to each other");
    }
}
