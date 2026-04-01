use std::collections::{HashMap, HashSet, VecDeque};

use libp2p::PeerId;
use sha2::{Digest, Sha256};

use crate::mesh::Mesh;
use crate::scoring::PeerScores;

/// Maximum number of message IDs retained in the seen set before eviction.
const MAX_SEEN_MESSAGES: usize = 100_000;

/// Maximum number of delivered messages retained per topic.
const MAX_DELIVERED_PER_TOPIC: usize = 10_000;

#[derive(Default)]
pub struct GossipRouter {
    mesh: Mesh,
    scores: PeerScores,
    seen: HashSet<[u8; 32]>,
    /// FIFO order for evicting oldest entries from `seen`.
    seen_order: VecDeque<[u8; 32]>,
    delivered: HashMap<String, VecDeque<Vec<u8>>>,
}

pub struct GossipOutcome {
    pub delivered: bool,
    pub forwarded_to: Vec<PeerId>,
}

impl GossipRouter {
    pub fn new() -> Self {
        GossipRouter::default()
    }

    pub fn mesh_mut(&mut self) -> &mut Mesh {
        &mut self.mesh
    }

    pub fn delivered_messages(&self, topic: &str) -> Vec<&[u8]> {
        self.delivered
            .get(topic)
            .map(|msgs| msgs.iter().map(|m| m.as_slice()).collect())
            .unwrap_or_default()
    }

    pub fn publish(&mut self, topic: &str, data: Vec<u8>) -> GossipOutcome {
        let id = Self::message_id(topic, &data);
        if !self.insert_seen(id) {
            return GossipOutcome {
                delivered: false,
                forwarded_to: Vec::new(),
            };
        }

        let peers = self.mesh.peers(topic);
        for peer in &peers {
            self.scores.record_success(peer);
        }
        self.push_delivered(topic, data);

        GossipOutcome {
            delivered: true,
            forwarded_to: peers,
        }
    }

    pub fn receive(&mut self, from: &PeerId, topic: &str, data: Vec<u8>) -> GossipOutcome {
        let id = Self::message_id(topic, &data);
        if !self.insert_seen(id) {
            self.scores.record_failure(from);
            return GossipOutcome {
                delivered: false,
                forwarded_to: Vec::new(),
            };
        }

        self.scores.record_success(from);
        let peers: Vec<_> = self
            .mesh
            .peers(topic)
            .into_iter()
            .filter(|peer| peer != from)
            .collect();

        self.push_delivered(topic, data);

        GossipOutcome {
            delivered: true,
            forwarded_to: peers,
        }
    }

    /// Insert a message ID into `seen`, evicting the oldest half when the cap
    /// is reached. Returns `true` if the ID was new.
    fn insert_seen(&mut self, id: [u8; 32]) -> bool {
        if !self.seen.insert(id) {
            return false;
        }
        self.seen_order.push_back(id);
        if self.seen.len() > MAX_SEEN_MESSAGES {
            let to_remove = self.seen.len() / 2;
            for _ in 0..to_remove {
                if let Some(old) = self.seen_order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
        true
    }

    /// Push a delivered message, evicting oldest when the per-topic cap is hit.
    fn push_delivered(&mut self, topic: &str, data: Vec<u8>) {
        let queue = self.delivered.entry(topic.to_string()).or_default();
        queue.push_back(data);
        while queue.len() > MAX_DELIVERED_PER_TOPIC {
            queue.pop_front();
        }
    }

    fn message_id(topic: &str, data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(topic.as_bytes());
        hasher.update(data);
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_and_receive() {
        let mut router = GossipRouter::new();
        let peer = PeerId::random();
        router.mesh_mut().join("tx", peer);

        let outcome = router.publish("tx", b"hello".to_vec());
        assert!(outcome.delivered);
        assert_eq!(outcome.forwarded_to.len(), 1);

        let outcome = router.receive(&peer, "tx", b"world".to_vec());
        assert!(outcome.delivered);
        assert!(outcome.forwarded_to.is_empty());
    }

    #[test]
    fn seen_set_evicts_oldest_when_full() {
        let mut router = GossipRouter::new();
        // Insert MAX_SEEN_MESSAGES + 1 unique messages to trigger eviction.
        for i in 0..=MAX_SEEN_MESSAGES {
            let data = i.to_le_bytes().to_vec();
            router.publish("tx", data);
        }
        // After eviction, seen set should be roughly half the cap.
        assert!(
            router.seen.len() <= MAX_SEEN_MESSAGES,
            "seen set should have been evicted, got {}",
            router.seen.len()
        );
    }

    #[test]
    fn delivered_evicts_oldest_when_full() {
        let mut router = GossipRouter::new();
        for i in 0..MAX_DELIVERED_PER_TOPIC + 100 {
            let data = i.to_le_bytes().to_vec();
            router.publish("tx", data);
        }
        let msgs = router.delivered_messages("tx");
        assert!(
            msgs.len() <= MAX_DELIVERED_PER_TOPIC,
            "delivered should be capped at {}, got {}",
            MAX_DELIVERED_PER_TOPIC,
            msgs.len()
        );
    }
}
