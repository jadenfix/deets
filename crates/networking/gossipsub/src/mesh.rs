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

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Joining n distinct peers on a topic yields exactly n peers.
        #[test]
        fn join_n_peers_yields_n(n in 0usize..64usize) {
            let mut mesh = Mesh::new();
            let peers: Vec<_> = (0..n).map(|_| PeerId::random()).collect();
            for &p in &peers {
                mesh.join("tx", p);
            }
            prop_assert_eq!(mesh.peers("tx").len(), n);
        }

        /// Joining the same peer twice on a topic still yields exactly 1 peer (set semantics).
        #[test]
        fn join_idempotent(_dummy in 0u8..10u8) {
            let mut mesh = Mesh::new();
            let peer = PeerId::random();
            mesh.join("tx", peer);
            mesh.join("tx", peer);
            prop_assert_eq!(mesh.peers("tx").len(), 1);
        }

        /// After leaving, the peer is no longer in the topic's peer list.
        #[test]
        fn leave_removes_peer(n in 1usize..32usize) {
            let mut mesh = Mesh::new();
            let peers: Vec<_> = (0..n).map(|_| PeerId::random()).collect();
            for &p in &peers {
                mesh.join("blk", p);
            }
            let target = peers[0];
            mesh.leave("blk", &target);
            prop_assert!(!mesh.peers("blk").contains(&target));
        }

        /// Peers on different topics are independent (join on "tx" does not affect "blk").
        #[test]
        fn topics_are_independent(n in 1usize..16usize) {
            let mut mesh = Mesh::new();
            let peers: Vec<_> = (0..n).map(|_| PeerId::random()).collect();
            for &p in &peers {
                mesh.join("tx", p);
            }
            prop_assert_eq!(mesh.peers("blk").len(), 0);
        }

        /// After all peers leave a topic, the topic disappears from the iterator.
        #[test]
        fn empty_topic_removed(n in 1usize..16usize) {
            let mut mesh = Mesh::new();
            let peers: Vec<_> = (0..n).map(|_| PeerId::random()).collect();
            for &p in &peers {
                mesh.join("tx", p);
            }
            for p in &peers {
                mesh.leave("tx", p);
            }
            let topic_count = mesh.topics().count();
            prop_assert_eq!(topic_count, 0);
        }

        /// Leaving a non-existent peer is a no-op (no panic).
        #[test]
        fn leave_nonexistent_is_noop(_dummy in 0u8..10u8) {
            let mut mesh = Mesh::new();
            let peer = PeerId::random();
            // Should not panic
            mesh.leave("tx", &peer);
            prop_assert_eq!(mesh.peers("tx").len(), 0);
        }
    }
}
