use std::collections::{HashMap, HashSet, VecDeque};

use libp2p::PeerId;
use sha2::{Digest, Sha256};

use crate::mesh::Mesh;
use crate::scoring::PeerScores;

/// Maximum number of message IDs retained for deduplication.
/// At ~32 bytes per entry, 100k entries ≈ 3.2 MB.
const MAX_SEEN_CACHE: usize = 100_000;

/// Maximum number of delivered message payloads retained per topic.
const MAX_DELIVERED_PER_TOPIC: usize = 10_000;

#[derive(Default)]
pub struct GossipRouter {
    mesh: Mesh,
    scores: PeerScores,
    seen: HashSet<[u8; 32]>,
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

    pub fn delivered_messages(&self, topic: &str) -> Vec<&Vec<u8>> {
        self.delivered
            .get(topic)
            .map(|msgs| msgs.iter().collect())
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

    /// Insert a message ID into the seen cache, evicting the oldest if at capacity.
    /// Returns true if the ID was new (not previously seen).
    fn insert_seen(&mut self, id: [u8; 32]) -> bool {
        if !self.seen.insert(id) {
            return false;
        }
        self.seen_order.push_back(id);
        while self.seen.len() > MAX_SEEN_CACHE {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen.remove(&old);
            }
        }
        true
    }

    /// Push a delivered message, evicting oldest if per-topic cap is reached.
    fn push_delivered(&mut self, topic: &str, data: Vec<u8>) {
        let queue = self.delivered.entry(topic.to_string()).or_default();
        queue.push_back(data);
        while queue.len() > MAX_DELIVERED_PER_TOPIC {
            queue.pop_front();
        }
    }

    pub(crate) fn message_id(topic: &str, data: &[u8]) -> [u8; 32] {
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
    fn seen_cache_evicts_oldest() {
        let mut router = GossipRouter::new();

        // Fill seen cache beyond MAX_SEEN_CACHE
        for i in 0..MAX_SEEN_CACHE + 100 {
            let data = format!("msg-{}", i).into_bytes();
            router.publish("tx", data);
        }

        // seen set should be bounded
        assert!(
            router.seen.len() <= MAX_SEEN_CACHE,
            "seen cache should be bounded to {}, got {}",
            MAX_SEEN_CACHE,
            router.seen.len()
        );

        // Oldest messages should now be re-deliverable (evicted from seen)
        let outcome = router.publish("tx", b"msg-0".to_vec());
        assert!(
            outcome.delivered,
            "evicted message should be deliverable again"
        );
    }

    #[test]
    fn delivered_per_topic_bounded() {
        let mut router = GossipRouter::new();

        for i in 0..MAX_DELIVERED_PER_TOPIC + 500 {
            let data = format!("payload-{}", i).into_bytes();
            router.publish("block", data);
        }

        let msgs = router.delivered_messages("block");
        assert!(
            msgs.len() <= MAX_DELIVERED_PER_TOPIC,
            "delivered should be bounded to {}, got {}",
            MAX_DELIVERED_PER_TOPIC,
            msgs.len()
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Publishing the same message twice: first is delivered, second is not (dedup).
        #[test]
        fn duplicate_publish_rejected(data in prop::collection::vec(any::<u8>(), 1..128usize)) {
            let mut router = GossipRouter::new();
            let o1 = router.publish("tx", data.clone());
            let o2 = router.publish("tx", data.clone());
            prop_assert!(o1.delivered);
            prop_assert!(!o2.delivered);
        }

        /// Receiving a duplicate from a peer: first delivered, second rejected with peer penalised.
        #[test]
        fn duplicate_receive_penalises_sender(data in prop::collection::vec(any::<u8>(), 1..128usize)) {
            let mut router = GossipRouter::new();
            let peer = PeerId::random();
            let o1 = router.receive(&peer, "tx", data.clone());
            let o2 = router.receive(&peer, "tx", data.clone());
            prop_assert!(o1.delivered);
            prop_assert!(!o2.delivered);
        }

        /// A published message is forwarded only to mesh peers of that topic.
        #[test]
        fn forward_only_to_mesh_peers(n_peers in 0usize..10usize) {
            let mut router = GossipRouter::new();
            for _ in 0..n_peers {
                router.mesh_mut().join("blk", PeerId::random());
            }
            let outcome = router.publish("blk", b"data".to_vec());
            prop_assert_eq!(outcome.forwarded_to.len(), n_peers);
        }

        /// A received message is NOT forwarded back to the sender.
        #[test]
        fn receive_does_not_echo_to_sender(n_extra in 0usize..8usize) {
            let mut router = GossipRouter::new();
            let sender = PeerId::random();
            router.mesh_mut().join("tx", sender);
            for _ in 0..n_extra {
                router.mesh_mut().join("tx", PeerId::random());
            }
            let outcome = router.receive(&sender, "tx", b"msg".to_vec());
            prop_assert!(outcome.delivered);
            prop_assert!(!outcome.forwarded_to.contains(&sender));
        }

        /// Delivered message queue per topic never exceeds MAX_DELIVERED_PER_TOPIC.
        #[test]
        fn delivered_queue_bounded_proptest(extra in 0usize..200usize) {
            let mut router = GossipRouter::new();
            for i in 0..MAX_DELIVERED_PER_TOPIC + extra {
                router.publish("t", format!("msg{}", i).into_bytes());
            }
            prop_assert!(router.delivered_messages("t").len() <= MAX_DELIVERED_PER_TOPIC);
        }

        /// Seen cache never exceeds MAX_SEEN_CACHE.
        #[test]
        fn seen_cache_bounded_proptest(extra in 0usize..500usize) {
            let mut router = GossipRouter::new();
            for i in 0..MAX_SEEN_CACHE + extra {
                router.publish("t", format!("unique-{}", i).into_bytes());
            }
            prop_assert!(router.seen.len() <= MAX_SEEN_CACHE);
        }

        /// message_id is deterministic: same topic + data always yields the same hash.
        #[test]
        fn message_id_deterministic(
            topic in "[a-z]{1,10}",
            data in prop::collection::vec(any::<u8>(), 0..64usize),
        ) {
            let id1 = GossipRouter::message_id(&topic, &data);
            let id2 = GossipRouter::message_id(&topic, &data);
            prop_assert_eq!(id1, id2);
        }

        /// Different data under the same topic produces different message IDs (collision resistance).
        #[test]
        fn message_id_differs_for_different_data(
            d1 in prop::collection::vec(any::<u8>(), 1..32usize),
            d2 in prop::collection::vec(any::<u8>(), 1..32usize),
        ) {
            prop_assume!(d1 != d2);
            let id1 = GossipRouter::message_id("t", &d1);
            let id2 = GossipRouter::message_id("t", &d2);
            prop_assert_ne!(id1, id2);
        }
    }
}
