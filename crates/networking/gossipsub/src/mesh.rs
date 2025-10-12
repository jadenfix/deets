use std::collections::{HashMap, HashSet};

use libp2p::PeerId;

#[derive(Default, Debug, Clone)]
pub struct Mesh {
    topics: HashMap<String, HashSet<PeerId>>,
}

impl Mesh {
    pub fn new() -> Self {
        Mesh {
            topics: HashMap::new(),
        }
    }

    pub fn join(&mut self, topic: &str, peer: PeerId) {
        self.topics
            .entry(topic.to_string())
            .or_default()
            .insert(peer);
    }

    pub fn leave(&mut self, topic: &str, peer: &PeerId) {
        if let Some(peers) = self.topics.get_mut(topic) {
            peers.remove(peer);
            if peers.is_empty() {
                self.topics.remove(topic);
            }
        }
    }

    pub fn peers(&self, topic: &str) -> Vec<PeerId> {
        self.topics
            .get(topic)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn topics(&self) -> impl Iterator<Item = (&String, &HashSet<PeerId>)> {
        self.topics.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_and_leave() {
        let mut mesh = Mesh::new();
        let peer = PeerId::random();
        mesh.join("tx", peer);
        assert_eq!(mesh.peers("tx").len(), 1);
        mesh.leave("tx", &peer);
        assert!(mesh.peers("tx").is_empty());
    }
}
