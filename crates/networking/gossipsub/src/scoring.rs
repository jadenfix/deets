use std::collections::HashMap;

use libp2p::PeerId;

/// Maximum number of peers tracked in the scoring table.
/// Prevents unbounded memory growth from Sybil peers connecting and
/// disconnecting repeatedly. When the cap is reached, the lowest-scoring
/// peer is evicted to make room.
const MAX_TRACKED_PEERS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeerScore {
    pub score: f64,
}

impl PeerScore {
    pub fn new() -> Self {
        PeerScore { score: 0.0 }
    }

    pub fn apply_success(&mut self) {
        self.score += 1.0;
        if self.score > 10.0 {
            self.score = 10.0;
        }
    }

    pub fn apply_failure(&mut self) {
        self.score -= 2.0;
        if self.score < -10.0 {
            self.score = -10.0;
        }
    }

    pub fn decay(&mut self) {
        self.score *= 0.9;
    }
}

impl Default for PeerScore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default, Debug)]
pub struct PeerScores {
    scores: HashMap<PeerId, PeerScore>,
}

impl PeerScores {
    pub fn new() -> Self {
        PeerScores {
            scores: HashMap::new(),
        }
    }

    pub fn record_success(&mut self, peer: &PeerId) {
        self.ensure_capacity(peer);
        self.scores.entry(*peer).or_default().apply_success();
    }

    pub fn record_failure(&mut self, peer: &PeerId) {
        self.ensure_capacity(peer);
        self.scores.entry(*peer).or_default().apply_failure();
    }

    pub fn score(&self, peer: &PeerId) -> f64 {
        self.scores.get(peer).map(|s| s.score).unwrap_or(0.0)
    }

    /// Remove a peer's score entry (e.g., on disconnect).
    pub fn remove_peer(&mut self, peer: &PeerId) {
        self.scores.remove(peer);
    }

    /// Number of tracked peers.
    pub fn len(&self) -> usize {
        self.scores.len()
    }

    /// Returns true if no peers are tracked.
    pub fn is_empty(&self) -> bool {
        self.scores.is_empty()
    }

    /// If the peer is not already tracked and we are at capacity, evict the
    /// lowest-scoring peer to make room. This bounds memory at
    /// `MAX_TRACKED_PEERS` entries even under Sybil attack.
    fn ensure_capacity(&mut self, peer: &PeerId) {
        if self.scores.contains_key(peer) || self.scores.len() < MAX_TRACKED_PEERS {
            return;
        }
        // Evict the peer with the lowest score.
        if let Some((&worst_peer, _)) = self.scores.iter().min_by(|a, b| {
            a.1.score
                .partial_cmp(&b.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            self.scores.remove(&worst_peer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_updates() {
        let peer = PeerId::random();
        let mut scores = PeerScores::new();
        scores.record_success(&peer);
        scores.record_failure(&peer);
        assert!(scores.score(&peer) < 1.0);
    }

    #[test]
    fn evicts_lowest_score_at_capacity() {
        let mut scores = PeerScores::new();

        // Fill to capacity
        let mut peers = Vec::new();
        for _ in 0..MAX_TRACKED_PEERS {
            let p = PeerId::random();
            scores.record_success(&p);
            peers.push(p);
        }
        assert_eq!(scores.len(), MAX_TRACKED_PEERS);

        // Give one peer the worst score
        let victim = peers[0];
        for _ in 0..10 {
            scores.record_failure(&victim);
        }

        // Add a new peer — should evict the lowest-scoring peer
        let newcomer = PeerId::random();
        scores.record_success(&newcomer);
        assert_eq!(scores.len(), MAX_TRACKED_PEERS);
        assert!(scores.score(&newcomer) > 0.0);
        // The victim (lowest score) should have been evicted
        assert_eq!(scores.score(&victim), 0.0); // 0.0 = not found
    }

    #[test]
    fn existing_peer_does_not_trigger_eviction() {
        let mut scores = PeerScores::new();
        let mut peers = Vec::new();
        for _ in 0..MAX_TRACKED_PEERS {
            let p = PeerId::random();
            scores.record_success(&p);
            peers.push(p);
        }
        // Recording against existing peer should not evict anyone
        scores.record_success(&peers[0]);
        assert_eq!(scores.len(), MAX_TRACKED_PEERS);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Score after any sequence of successes is bounded above by 10.0.
        #[test]
        fn success_score_capped(n in 1u32..200u32) {
            let peer = PeerId::random();
            let mut scores = PeerScores::new();
            for _ in 0..n {
                scores.record_success(&peer);
            }
            prop_assert!(scores.score(&peer) <= 10.0);
        }

        /// Score after any sequence of failures is bounded below by -10.0.
        #[test]
        fn failure_score_floored(n in 1u32..200u32) {
            let peer = PeerId::random();
            let mut scores = PeerScores::new();
            for _ in 0..n {
                scores.record_failure(&peer);
            }
            prop_assert!(scores.score(&peer) >= -10.0);
        }

        /// Each success increments the score (until cap).
        #[test]
        fn single_success_increases_score(initial_successes in 0u32..5u32) {
            let peer = PeerId::random();
            let mut scores = PeerScores::new();
            for _ in 0..initial_successes {
                scores.record_success(&peer);
            }
            let before = scores.score(&peer);
            scores.record_success(&peer);
            let after = scores.score(&peer);
            // Either it increased or it was already at cap (10.0)
            prop_assert!(after >= before);
        }

        /// Each failure decrements the score (until floor).
        #[test]
        fn single_failure_decreases_score(initial_failures in 0u32..3u32) {
            let peer = PeerId::random();
            let mut scores = PeerScores::new();
            for _ in 0..initial_failures {
                scores.record_failure(&peer);
            }
            let before = scores.score(&peer);
            scores.record_failure(&peer);
            let after = scores.score(&peer);
            // Either it decreased or it was already at floor (-10.0)
            prop_assert!(after <= before);
        }

        /// After remove_peer, score returns 0.0 (default/absent).
        #[test]
        fn remove_peer_clears_score(successes in 1u32..10u32) {
            let peer = PeerId::random();
            let mut scores = PeerScores::new();
            for _ in 0..successes {
                scores.record_success(&peer);
            }
            prop_assert!(scores.score(&peer) > 0.0);
            scores.remove_peer(&peer);
            prop_assert_eq!(scores.score(&peer), 0.0);
        }

        /// Decay always moves score toward zero.
        #[test]
        fn decay_moves_toward_zero(successes in 1u32..10u32) {
            let peer = PeerId::random();
            let mut scores = PeerScores::new();
            for _ in 0..successes {
                scores.record_success(&peer);
            }
            let before = scores.score(&peer);
            if let Some(s) = scores.scores.get_mut(&peer) {
                s.decay();
            }
            let after = scores.score(&peer);
            // After decay of positive score, should be smaller magnitude
            prop_assert!(after.abs() <= before.abs());
        }

        /// len() never exceeds MAX_TRACKED_PEERS regardless of how many distinct peers are added.
        #[test]
        fn tracked_peers_bounded(extra in 0usize..100usize) {
            let mut scores = PeerScores::new();
            for _ in 0..MAX_TRACKED_PEERS + extra {
                let p = PeerId::random();
                scores.record_success(&p);
            }
            prop_assert!(scores.len() <= MAX_TRACKED_PEERS);
        }

        /// An unknown peer always returns score 0.0.
        #[test]
        fn unknown_peer_score_is_zero(_dummy in 0u8..10u8) {
            let scores = PeerScores::new();
            let peer = PeerId::random();
            prop_assert_eq!(scores.score(&peer), 0.0);
        }
    }
}
