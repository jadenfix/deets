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
        if let Some((&worst_peer, _)) = self
            .scores
            .iter()
            .min_by(|a, b| a.1.score.partial_cmp(&b.1.score).unwrap_or(std::cmp::Ordering::Equal))
        {
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
