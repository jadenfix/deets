use std::collections::{HashMap, HashSet};

use libp2p::PeerId;
use sha2::{Digest, Sha256};

use crate::mesh::Mesh;
use crate::scoring::PeerScores;

#[derive(Default)]
pub struct GossipRouter {
    mesh: Mesh,
    scores: PeerScores,
    seen: HashSet<[u8; 32]>,
    delivered: HashMap<String, Vec<Vec<u8>>>,
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

    pub fn delivered_messages(&self, topic: &str) -> &[Vec<u8>] {
        self.delivered
            .get(topic)
            .map(|msgs| msgs.as_slice())
            .unwrap_or(&[])
    }

    pub fn publish(&mut self, topic: &str, data: Vec<u8>) -> GossipOutcome {
        let id = Self::message_id(topic, &data);
        if !self.seen.insert(id) {
            return GossipOutcome {
                delivered: false,
                forwarded_to: Vec::new(),
            };
        }

        let peers = self.mesh.peers(topic);
        for peer in &peers {
            self.scores.record_success(peer);
        }
        self.delivered
            .entry(topic.to_string())
            .or_default()
            .push(data);

        GossipOutcome {
            delivered: true,
            forwarded_to: peers,
        }
    }

    pub fn receive(&mut self, from: &PeerId, topic: &str, data: Vec<u8>) -> GossipOutcome {
        let id = Self::message_id(topic, &data);
        if !self.seen.insert(id) {
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

        self.delivered
            .entry(topic.to_string())
            .or_default()
            .push(data);

        GossipOutcome {
            delivered: true,
            forwarded_to: peers,
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
}
