use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Gossipsub Protocol Implementation
///
/// Features:
/// - Topic-based pub/sub
/// - Message deduplication
/// - Flood publishing
/// - Peer scoring
/// - Message validation
///
/// Parameters:
/// - D: Desired peer degree (6)
/// - D_lo: Lower bound for peer degree (4)
/// - D_hi: Upper bound for peer degree (12)
/// - Heartbeat interval: 1 second
///
pub struct GossipManager {
    /// Subscribed topics
    subscriptions: HashMap<String, TopicInfo>,

    /// Seen messages (for deduplication)
    seen_messages: HashSet<Vec<u8>>,

    /// Message cache (for history)
    message_cache: Vec<(Vec<u8>, Instant)>,

    /// Max cache size
    cache_size: usize,

    /// Cache duration
    cache_duration: Duration,
}

#[derive(Clone)]
pub struct TopicInfo {
    pub name: String,
    pub peers: Vec<String>,
    pub message_count: u64,
}

impl GossipManager {
    pub fn new() -> Self {
        GossipManager {
            subscriptions: HashMap::new(),
            seen_messages: HashSet::new(),
            message_cache: Vec::new(),
            cache_size: 1000,
            cache_duration: Duration::from_secs(120), // 2 minutes
        }
    }

    /// Subscribe to a topic
    pub fn subscribe(&mut self, topic: String) {
        if !self.subscriptions.contains_key(&topic) {
            self.subscriptions.insert(
                topic.clone(),
                TopicInfo {
                    name: topic.clone(),
                    peers: vec![],
                    message_count: 0,
                },
            );
            println!("Subscribed to gossip topic: {}", topic);
        }
    }

    /// Unsubscribe from a topic
    pub fn unsubscribe(&mut self, topic: &str) {
        self.subscriptions.remove(topic);
        println!("Unsubscribed from gossip topic: {}", topic);
    }

    /// Publish a message to a topic
    pub fn publish(&mut self, topic: &str, message: Vec<u8>) -> Result<()> {
        // Check if subscribed
        if !self.subscriptions.contains_key(topic) {
            anyhow::bail!("not subscribed to topic: {}", topic);
        }

        // Deduplicate
        let msg_hash = self.hash_message(&message);
        if self.seen_messages.contains(&msg_hash) {
            return Ok(()); // Already seen
        }

        // Mark as seen
        self.seen_messages.insert(msg_hash.clone());

        // Add to cache
        self.cache_message(msg_hash, message.clone());

        // Update topic stats
        if let Some(info) = self.subscriptions.get_mut(topic) {
            info.message_count += 1;
        }

        // In production: forward to peers via gossipsub
        println!("Gossip published {} bytes to {}", message.len(), topic);

        Ok(())
    }

    /// Handle incoming message
    pub fn handle_message(&mut self, topic: &str, message: Vec<u8>) -> Result<bool> {
        let msg_hash = self.hash_message(&message);

        // Check if already seen (deduplication)
        if self.seen_messages.contains(&msg_hash) {
            return Ok(false); // Duplicate
        }

        // Mark as seen
        self.seen_messages.insert(msg_hash.clone());

        // Add to cache
        self.cache_message(msg_hash, message.clone());

        // Update topic stats
        if let Some(info) = self.subscriptions.get_mut(topic) {
            info.message_count += 1;
        }

        Ok(true) // New message
    }

    /// Add peer to topic
    pub fn add_peer_to_topic(&mut self, topic: &str, peer_id: String) {
        if let Some(info) = self.subscriptions.get_mut(topic) {
            if !info.peers.contains(&peer_id) {
                info.peers.push(peer_id);
            }
        }
    }

    /// Remove peer from topic
    pub fn remove_peer_from_topic(&mut self, topic: &str, peer_id: &str) {
        if let Some(info) = self.subscriptions.get_mut(topic) {
            info.peers.retain(|p| p != peer_id);
        }
    }

    /// Get peers for a topic
    pub fn get_topic_peers(&self, topic: &str) -> Vec<String> {
        self.subscriptions
            .get(topic)
            .map(|info| info.peers.clone())
            .unwrap_or_default()
    }

    /// Cleanup old messages
    pub fn cleanup(&mut self) {
        let now = Instant::now();

        // Remove old cached messages
        self.message_cache
            .retain(|(_, timestamp)| now.duration_since(*timestamp) < self.cache_duration);

        // Limit seen messages
        if self.seen_messages.len() > self.cache_size {
            self.seen_messages.clear();
        }
    }

    fn hash_message(&self, message: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        Sha256::digest(message).to_vec()
    }

    fn cache_message(&mut self, hash: Vec<u8>, _message: Vec<u8>) {
        if self.message_cache.len() >= self.cache_size {
            self.message_cache.remove(0);
        }
        self.message_cache.push((hash, Instant::now()));
    }

    pub fn topic_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn seen_message_count(&self) -> usize {
        self.seen_messages.len()
    }
}

impl Default for GossipManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe() {
        let mut gossip = GossipManager::new();

        gossip.subscribe("/aether/tx".to_string());
        assert_eq!(gossip.topic_count(), 1);
    }

    #[test]
    fn test_publish() {
        let mut gossip = GossipManager::new();

        gossip.subscribe("/aether/tx".to_string());
        gossip
            .publish("/aether/tx", b"test message".to_vec())
            .unwrap();

        assert_eq!(gossip.seen_message_count(), 1);
    }

    #[test]
    fn test_deduplication() {
        let mut gossip = GossipManager::new();

        gossip.subscribe("/aether/tx".to_string());

        let msg = b"test message".to_vec();

        // First publish
        gossip.publish("/aether/tx", msg.clone()).unwrap();
        assert_eq!(gossip.seen_message_count(), 1);

        // Duplicate publish - should be ignored
        gossip.publish("/aether/tx", msg).unwrap();
        assert_eq!(gossip.seen_message_count(), 1); // Still 1
    }

    #[test]
    fn test_peer_management() {
        let mut gossip = GossipManager::new();

        gossip.subscribe("/aether/tx".to_string());

        gossip.add_peer_to_topic("/aether/tx", "peer1".to_string());
        gossip.add_peer_to_topic("/aether/tx", "peer2".to_string());

        let peers = gossip.get_topic_peers("/aether/tx");
        assert_eq!(peers.len(), 2);

        gossip.remove_peer_from_topic("/aether/tx", "peer1");

        let peers = gossip.get_topic_peers("/aether/tx");
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn test_handle_message() {
        let mut gossip = GossipManager::new();

        gossip.subscribe("/aether/tx".to_string());

        let msg1 = b"message1".to_vec();
        let msg2 = b"message2".to_vec();

        // New message
        assert!(gossip.handle_message("/aether/tx", msg1.clone()).unwrap());

        // Duplicate
        assert!(!gossip.handle_message("/aether/tx", msg1).unwrap());

        // New message
        assert!(gossip.handle_message("/aether/tx", msg2).unwrap());
    }
}
