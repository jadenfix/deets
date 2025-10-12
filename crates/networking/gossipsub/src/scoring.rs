use std::collections::HashMap;

use libp2p::PeerId;

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
        self.scores.entry(*peer).or_default().apply_success();
    }

    pub fn record_failure(&mut self, peer: &PeerId) {
        self.scores.entry(*peer).or_default().apply_failure();
    }

    pub fn score(&self, peer: &PeerId) -> f64 {
        self.scores.get(peer).map(|s| s.score).unwrap_or(0.0)
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
}
