use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Maximum number of tracked transactions to prevent unbounded memory growth.
const MAX_TRACKED_TXS: usize = 50_000;

/// Dandelion++ transaction propagation for sender privacy.
///
/// Without Dandelion++, an adversary monitoring the gossip network can
/// identify the originating node of a transaction (and thus link it
/// to the sender's IP address).
///
/// Dandelion++ has two phases:
/// 1. **Stem phase**: The transaction is forwarded along a random path
///    (one hop at a time) for a random number of hops.
/// 2. **Fluff phase**: After the stem phase, the transaction is broadcast
///    via normal gossipsub flooding.
///
/// This makes it difficult to determine which node originated the tx.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropagationPhase {
    /// Forward to exactly one random peer (stem).
    Stem,
    /// Broadcast to all peers via gossipsub (fluff).
    Fluff,
}

/// Tracks the propagation state of a transaction.
#[derive(Debug)]
struct TxPropagation {
    phase: PropagationPhase,
    stem_hops_remaining: u32,
    created_at: Instant,
}

/// Dandelion++ propagation manager.
pub struct DandelionManager {
    /// Tx hash → propagation state
    states: HashMap<Vec<u8>, TxPropagation>,
    /// Number of stem hops before fluffing (randomized per tx).
    min_stem_hops: u32,
    max_stem_hops: u32,
    /// Maximum time in stem phase before auto-fluffing (failsafe).
    stem_timeout: Duration,
    /// Probability of fluffing at each stem hop (0.0-1.0).
    /// Higher = shorter stem phase on average.
    fluff_probability: f64,
}

impl DandelionManager {
    pub fn new() -> Self {
        DandelionManager {
            states: HashMap::new(),
            min_stem_hops: 1,
            max_stem_hops: 4,
            stem_timeout: Duration::from_secs(10),
            fluff_probability: 0.25,
        }
    }

    /// Determine the propagation phase for a transaction.
    ///
    /// - If the tx is new (originated locally), start in Stem phase.
    /// - If the tx is in Stem phase and has hops remaining, stay in Stem.
    /// - If stem hops exhausted or timeout reached, switch to Fluff.
    pub fn get_phase(&mut self, tx_hash: &[u8]) -> PropagationPhase {
        if let Some(state) = self.states.get_mut(tx_hash) {
            // Check timeout failsafe
            if state.created_at.elapsed() >= self.stem_timeout {
                state.phase = PropagationPhase::Fluff;
                return PropagationPhase::Fluff;
            }

            match state.phase {
                PropagationPhase::Stem => {
                    if state.stem_hops_remaining == 0 {
                        state.phase = PropagationPhase::Fluff;
                        PropagationPhase::Fluff
                    } else {
                        // Random chance of fluffing early
                        let r: f64 = rand_float(tx_hash);
                        if r < self.fluff_probability {
                            state.phase = PropagationPhase::Fluff;
                            PropagationPhase::Fluff
                        } else {
                            state.stem_hops_remaining -= 1;
                            PropagationPhase::Stem
                        }
                    }
                }
                PropagationPhase::Fluff => PropagationPhase::Fluff,
            }
        } else {
            // New transaction — start in stem phase
            let hop_range = self.max_stem_hops.saturating_sub(self.min_stem_hops);
            let hops = if hop_range == 0 {
                self.min_stem_hops
            } else {
                self.min_stem_hops + (rand_u32(tx_hash) % (hop_range + 1))
            };

            // Evict expired entries if we've hit the size cap
            if self.states.len() >= MAX_TRACKED_TXS {
                self.cleanup(self.stem_timeout);
            }

            // If still at capacity after cleanup, skip stem for this tx
            if self.states.len() >= MAX_TRACKED_TXS {
                return PropagationPhase::Fluff;
            }

            self.states.insert(
                tx_hash.to_vec(),
                TxPropagation {
                    phase: PropagationPhase::Stem,
                    stem_hops_remaining: hops,
                    created_at: Instant::now(),
                },
            );
            PropagationPhase::Stem
        }
    }

    /// Mark a transaction as fluffed (broadcast to all peers).
    pub fn mark_fluffed(&mut self, tx_hash: &[u8]) {
        if let Some(state) = self.states.get_mut(tx_hash) {
            state.phase = PropagationPhase::Fluff;
        }
    }

    /// Receive a stem-phase tx from another peer.
    /// Determine if we should continue stemming or fluff.
    pub fn on_stem_receive(&mut self, tx_hash: &[u8]) -> PropagationPhase {
        self.get_phase(tx_hash)
    }

    /// Clean up old entries.
    pub fn cleanup(&mut self, max_age: Duration) {
        self.states
            .retain(|_, state| state.created_at.elapsed() < max_age);
    }

    pub fn tracked_count(&self) -> usize {
        self.states.len()
    }
}

impl Default for DandelionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash-based pseudo-random float in [0, 1), seeded by tx_hash + timestamp.
///
/// Uses SHA-256 for uniform distribution, mixing the tx hash with the current
/// timestamp to avoid deterministic outcomes for the same tx across calls.
fn rand_float(tx_hash: &[u8]) -> f64 {
    let bytes = rand_bytes(tx_hash);
    let val = u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    (val >> 11) as f64 / ((1u64 << 53) as f64)
}

/// Hash-based pseudo-random u32, seeded by tx_hash + timestamp.
fn rand_u32(tx_hash: &[u8]) -> u32 {
    let bytes = rand_bytes(tx_hash);
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn rand_bytes(tx_hash: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut hasher = Sha256::new();
    hasher.update(tx_hash);
    hasher.update(nanos.to_le_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tx_starts_in_stem() {
        let mut dm = DandelionManager::new();
        let phase = dm.get_phase(b"tx1");
        assert_eq!(phase, PropagationPhase::Stem);
    }

    #[test]
    fn test_eventually_fluffs() {
        let mut dm = DandelionManager::new();
        dm.fluff_probability = 1.0; // Always fluff on second call

        // First call: stem (new tx)
        let p1 = dm.get_phase(b"tx1");
        assert_eq!(p1, PropagationPhase::Stem);

        // Second call: should fluff (probability = 1.0)
        let p2 = dm.get_phase(b"tx1");
        assert_eq!(p2, PropagationPhase::Fluff);
    }

    #[test]
    fn test_stays_fluffed() {
        let mut dm = DandelionManager::new();
        dm.mark_fluffed(b"tx1");

        // Even after mark, a new get_phase for unknown tx starts stem
        // But for a known fluffed tx, it stays fluff
        dm.get_phase(b"tx2"); // Create stem entry for tx2
        dm.mark_fluffed(b"tx2");
        assert_eq!(dm.get_phase(b"tx2"), PropagationPhase::Fluff);
    }

    #[test]
    fn test_stem_timeout_triggers_fluff() {
        let mut dm = DandelionManager::new();
        dm.stem_timeout = Duration::from_millis(1);
        dm.fluff_probability = 0.0; // Never fluff randomly

        dm.get_phase(b"tx1"); // Create stem entry
        std::thread::sleep(Duration::from_millis(5));

        let phase = dm.get_phase(b"tx1");
        assert_eq!(
            phase,
            PropagationPhase::Fluff,
            "should auto-fluff after timeout"
        );
    }

    #[test]
    fn test_size_cap_triggers_fluff() {
        let mut dm = DandelionManager::new();
        dm.stem_timeout = Duration::from_secs(3600); // won't expire

        // Fill to capacity
        for i in 0..MAX_TRACKED_TXS {
            dm.get_phase(&i.to_be_bytes());
        }
        assert_eq!(dm.tracked_count(), MAX_TRACKED_TXS);

        // Next new tx should get Fluff (overflow protection)
        let phase = dm.get_phase(b"overflow_tx");
        assert_eq!(
            phase,
            PropagationPhase::Fluff,
            "should fluff when at capacity"
        );
    }

    #[test]
    fn test_cleanup() {
        let mut dm = DandelionManager::new();
        dm.get_phase(b"tx1");
        dm.get_phase(b"tx2");

        assert_eq!(dm.tracked_count(), 2);

        std::thread::sleep(Duration::from_millis(5));
        dm.cleanup(Duration::from_millis(1)); // Everything older than 1ms
        assert_eq!(dm.tracked_count(), 0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn new_tx_always_starts_stem(tx_hash in proptest::collection::vec(any::<u8>(), 1..64)) {
            let mut dm = DandelionManager::new();
            let phase = dm.get_phase(&tx_hash);
            prop_assert_eq!(phase, PropagationPhase::Stem);
        }

        #[test]
        fn fluff_is_absorbing(tx_hash in proptest::collection::vec(any::<u8>(), 1..32)) {
            let mut dm = DandelionManager::new();
            dm.get_phase(&tx_hash); // create entry
            dm.mark_fluffed(&tx_hash);
            // Once fluffed, must stay fluffed
            for _ in 0..10 {
                prop_assert_eq!(dm.get_phase(&tx_hash), PropagationPhase::Fluff);
            }
        }

        #[test]
        fn guaranteed_fluff_with_prob_one(tx_hash in proptest::collection::vec(any::<u8>(), 1..32)) {
            let mut dm = DandelionManager::new();
            dm.fluff_probability = 1.0;
            dm.get_phase(&tx_hash); // stem
            let phase = dm.get_phase(&tx_hash); // should fluff
            prop_assert_eq!(phase, PropagationPhase::Fluff);
        }

        #[test]
        fn zero_prob_exhausts_hops(tx_hash in proptest::collection::vec(any::<u8>(), 1..32)) {
            let mut dm = DandelionManager::new();
            dm.fluff_probability = 0.0;
            dm.min_stem_hops = 2;
            dm.max_stem_hops = 2; // exactly 2 hops

            dm.get_phase(&tx_hash); // creates with 2 hops
            prop_assert_eq!(dm.get_phase(&tx_hash), PropagationPhase::Stem); // hop 1 (1 remaining)
            prop_assert_eq!(dm.get_phase(&tx_hash), PropagationPhase::Stem); // hop 2 (0 remaining)
            prop_assert_eq!(dm.get_phase(&tx_hash), PropagationPhase::Fluff); // exhausted
        }

        #[test]
        fn cleanup_removes_all_old_entries(num_txs in 1usize..50) {
            let mut dm = DandelionManager::new();
            for i in 0..num_txs {
                dm.get_phase(&i.to_be_bytes());
            }
            prop_assert_eq!(dm.tracked_count(), num_txs);

            std::thread::sleep(Duration::from_millis(5));
            dm.cleanup(Duration::from_millis(1));
            prop_assert_eq!(dm.tracked_count(), 0);
        }

        #[test]
        fn tracked_count_matches_unique_txs(num_txs in 1usize..30) {
            let mut dm = DandelionManager::new();
            for i in 0..num_txs {
                dm.get_phase(&i.to_be_bytes());
            }
            prop_assert_eq!(dm.tracked_count(), num_txs);
            // Re-querying same txs doesn't increase count
            for i in 0..num_txs {
                dm.get_phase(&i.to_be_bytes());
            }
            prop_assert_eq!(dm.tracked_count(), num_txs);
        }

        #[test]
        fn eventual_fluff_within_max_hops(tx_hash in proptest::collection::vec(any::<u8>(), 1..32)) {
            let mut dm = DandelionManager::new();
            dm.fluff_probability = 0.0; // never random fluff
            dm.min_stem_hops = 1;
            dm.max_stem_hops = 4;

            dm.get_phase(&tx_hash); // create
            // After at most max_stem_hops + 1 calls, must be fluff
            let mut reached_fluff = false;
            for _ in 0..6 {
                if dm.get_phase(&tx_hash) == PropagationPhase::Fluff {
                    reached_fluff = true;
                    break;
                }
            }
            prop_assert!(reached_fluff, "must reach fluff within max_stem_hops + 1 calls");
        }
    }
}
