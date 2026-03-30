use std::time::{Duration, Instant};

/// Pacemaker drives round progression with exponential backoff timeouts.
///
/// When the current round's leader doesn't produce a block or enough votes
/// aren't collected, the pacemaker fires a timeout. Validators then
/// participate in a view-change protocol to elect a new leader.
pub struct Pacemaker {
    /// Base timeout for a round.
    base_timeout: Duration,
    /// Current timeout (increases on consecutive failures).
    current_timeout: Duration,
    /// Maximum timeout cap.
    max_timeout: Duration,
    /// When the current round started.
    round_start: Instant,
    /// Current round number (advances on timeout or successful commit).
    current_round: u64,
    /// Number of consecutive timeouts (reset on successful commit).
    consecutive_timeouts: u32,
}

impl Pacemaker {
    pub fn new(base_timeout: Duration) -> Self {
        Pacemaker {
            base_timeout,
            current_timeout: base_timeout,
            max_timeout: Duration::from_secs(30),
            round_start: Instant::now(),
            current_round: 0,
            consecutive_timeouts: 0,
        }
    }

    /// Check if the current round has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.round_start.elapsed() >= self.current_timeout
    }

    /// Get time remaining in the current round.
    pub fn time_remaining(&self) -> Duration {
        self.current_timeout
            .checked_sub(self.round_start.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    /// Advance to the next round due to a timeout.
    /// Doubles the timeout (exponential backoff) up to max.
    pub fn on_timeout(&mut self) {
        self.consecutive_timeouts += 1;
        self.current_round += 1;
        self.current_timeout = std::cmp::min(
            self.base_timeout * 2u32.pow(self.consecutive_timeouts.min(5)),
            self.max_timeout,
        );
        self.round_start = Instant::now();
    }

    /// Advance to the next round after a successful commit.
    /// Resets timeout to base.
    pub fn on_commit(&mut self) {
        self.consecutive_timeouts = 0;
        self.current_round += 1;
        self.current_timeout = self.base_timeout;
        self.round_start = Instant::now();
    }

    /// Reset the round timer (e.g., when receiving a valid proposal).
    pub fn reset_timer(&mut self) {
        self.round_start = Instant::now();
    }

    pub fn current_round(&self) -> u64 {
        self.current_round
    }

    pub fn current_timeout(&self) -> Duration {
        self.current_timeout
    }

    /// Determine which validator is the leader for a given round.
    /// Simple round-robin modulo validator count.
    pub fn leader_for_round(&self, round: u64, validator_count: usize) -> usize {
        if validator_count == 0 {
            return 0;
        }
        (round as usize) % validator_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pacemaker_creation() {
        let pm = Pacemaker::new(Duration::from_millis(500));
        assert_eq!(pm.current_round(), 0);
        assert_eq!(pm.current_timeout(), Duration::from_millis(500));
        assert!(!pm.is_timed_out());
    }

    #[test]
    fn test_timeout_doubles() {
        let mut pm = Pacemaker::new(Duration::from_millis(500));

        pm.on_timeout();
        assert_eq!(pm.current_round(), 1);
        assert_eq!(pm.current_timeout(), Duration::from_millis(1000));

        pm.on_timeout();
        assert_eq!(pm.current_round(), 2);
        assert_eq!(pm.current_timeout(), Duration::from_millis(2000));
    }

    #[test]
    fn test_timeout_caps_at_max() {
        let mut pm = Pacemaker::new(Duration::from_secs(1));
        // Timeout 10 times — should cap at 30s
        for _ in 0..10 {
            pm.on_timeout();
        }
        assert!(pm.current_timeout() <= Duration::from_secs(30));
    }

    #[test]
    fn test_commit_resets_timeout() {
        let mut pm = Pacemaker::new(Duration::from_millis(500));

        pm.on_timeout();
        pm.on_timeout();
        assert_eq!(pm.current_timeout(), Duration::from_millis(2000));

        pm.on_commit();
        assert_eq!(pm.current_timeout(), Duration::from_millis(500));
        assert_eq!(pm.consecutive_timeouts, 0);
    }

    #[test]
    fn test_leader_rotation() {
        let pm = Pacemaker::new(Duration::from_millis(500));
        assert_eq!(pm.leader_for_round(0, 4), 0);
        assert_eq!(pm.leader_for_round(1, 4), 1);
        assert_eq!(pm.leader_for_round(4, 4), 0);
        assert_eq!(pm.leader_for_round(7, 4), 3);
    }

    #[test]
    fn test_is_timed_out() {
        let pm = Pacemaker::new(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));
        assert!(pm.is_timed_out());
    }
}
