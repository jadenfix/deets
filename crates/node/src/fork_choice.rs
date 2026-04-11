use aether_types::{Slot, H256};
use std::collections::{HashMap, HashSet};

/// Maximum number of candidate blocks tracked per slot.
/// Prevents OOM from an attacker spamming unique block hashes for a single slot.
const MAX_CANDIDATES_PER_SLOT: usize = 16;

/// Simple fork choice: for each slot, track all candidate blocks and
/// select the canonical one based on finality or first-seen ordering.
pub struct ForkChoice {
    /// All known blocks per slot (can have multiple competing blocks).
    candidates: HashMap<Slot, Vec<H256>>,
    /// The chosen canonical block per slot.
    canonical: HashMap<Slot, H256>,
    /// Finalized blocks (immutable once set).
    finalized: HashMap<Slot, H256>,
    /// Slots whose canonical block has been committed to storage.
    /// Once committed, the canonical block for that slot is locked —
    /// a late-arriving fork cannot switch it without a full state rollback
    /// (which we don't support). This prevents state corruption from
    /// applying two competing blocks' overlays at the same slot.
    committed: HashSet<Slot>,
}

impl Default for ForkChoice {
    fn default() -> Self {
        Self::new()
    }
}

impl ForkChoice {
    pub fn new() -> Self {
        ForkChoice {
            candidates: HashMap::new(),
            canonical: HashMap::new(),
            finalized: HashMap::new(),
            committed: HashSet::new(),
        }
    }

    /// Record a new block candidate for a slot. Returns true if this is a new fork
    /// (competing block for an already-occupied slot).
    pub fn add_block(&mut self, slot: Slot, block_hash: H256) -> bool {
        // Reject new blocks for finalized or committed slots — once state is
        // committed to storage, switching canonical blocks would corrupt state
        // (both overlays applied without rollback).
        if self.finalized.contains_key(&slot) || self.committed.contains(&slot) {
            return false;
        }

        let candidates = self.candidates.entry(slot).or_default();
        if candidates.contains(&block_hash) {
            return false; // Already known
        }
        if candidates.len() >= MAX_CANDIDATES_PER_SLOT {
            return false; // Prevent OOM from excessive candidates per slot
        }
        let is_fork = !candidates.is_empty();
        candidates.push(block_hash);

        if is_fork {
            tracing::warn!(
                slot,
                block = ?block_hash,
                candidates = candidates.len(),
                "fork detected — competing block at same slot"
            );
        }

        // Deterministic tiebreak: prefer lower hash (compare raw bytes)
        match self.canonical.entry(slot) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(block_hash);
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if block_hash.as_bytes() < e.get().as_bytes() {
                    e.insert(block_hash);
                }
            }
        }

        is_fork
    }

    /// Get the canonical block for a slot.
    pub fn canonical_block(&self, slot: Slot) -> Option<H256> {
        self.finalized
            .get(&slot)
            .or_else(|| self.canonical.get(&slot))
            .copied()
    }

    /// Finalize a block at a slot. Only blocks that are known candidates
    /// (or already canonical) can be finalized.
    pub fn finalize(&mut self, slot: Slot, block_hash: H256) -> bool {
        // Only finalize blocks we've actually seen
        let is_known = self
            .candidates
            .get(&slot)
            .is_some_and(|c| c.contains(&block_hash));
        let is_canonical = self.canonical.get(&slot) == Some(&block_hash);

        if !is_known && !is_canonical {
            return false; // Reject unknown block hash
        }

        tracing::debug!(slot, block = ?block_hash, "fork choice finalized");
        self.finalized.insert(slot, block_hash);
        self.canonical.insert(slot, block_hash);
        true
    }

    /// Check if a slot has competing blocks (a fork).
    pub fn has_fork(&self, slot: Slot) -> bool {
        self.candidates.get(&slot).is_some_and(|c| c.len() > 1)
    }

    /// Get all candidate blocks for a slot.
    pub fn candidates_for(&self, slot: Slot) -> &[H256] {
        self.candidates.get(&slot).map_or(&[], |v| v.as_slice())
    }

    /// Check if a slot is finalized.
    pub fn is_finalized(&self, slot: Slot) -> bool {
        self.finalized.contains_key(&slot)
    }

    /// Mark a slot as having its canonical block's state committed to storage.
    /// After this, no fork-choice reorg is allowed for this slot.
    pub fn mark_committed(&mut self, slot: Slot) {
        self.committed.insert(slot);
    }

    /// Prune data for slots before `min_slot` to bound memory.
    pub fn prune_before(&mut self, min_slot: Slot) {
        self.candidates.retain(|&s, _| s >= min_slot);
        self.canonical.retain(|&s, _| s >= min_slot);
        self.finalized.retain(|&s, _| s >= min_slot);
        self.committed.retain(|s| *s >= min_slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(n: u8) -> H256 {
        H256::from_slice(&[n; 32]).unwrap()
    }

    #[test]
    fn single_block_per_slot() {
        let mut fc = ForkChoice::new();
        let is_fork = fc.add_block(1, hash(1));
        assert!(!is_fork);
        assert_eq!(fc.canonical_block(1), Some(hash(1)));
    }

    #[test]
    fn competing_blocks_detected() {
        let mut fc = ForkChoice::new();
        fc.add_block(1, hash(1));
        let is_fork = fc.add_block(1, hash(2));
        assert!(is_fork, "Second block at same slot should be a fork");
        assert!(fc.has_fork(1));
        // Lower hash wins (deterministic tiebreak)
        assert_eq!(fc.canonical_block(1), Some(hash(1)));
    }

    #[test]
    fn finalized_block_cannot_be_overridden() {
        let mut fc = ForkChoice::new();
        fc.add_block(1, hash(1));
        assert!(fc.finalize(1, hash(1)));

        // New competing block arrives after finalization
        fc.add_block(1, hash(2));
        // Canonical remains the finalized block
        assert_eq!(fc.canonical_block(1), Some(hash(1)));
        assert!(fc.is_finalized(1));
    }

    #[test]
    fn duplicate_block_ignored() {
        let mut fc = ForkChoice::new();
        fc.add_block(1, hash(1));
        let is_fork = fc.add_block(1, hash(1));
        assert!(!is_fork, "Duplicate block should not be a fork");
        assert_eq!(fc.candidates_for(1).len(), 1);
    }

    #[test]
    fn empty_slot_returns_none() {
        let fc = ForkChoice::new();
        assert_eq!(fc.canonical_block(99), None);
        assert!(!fc.has_fork(99));
    }

    #[test]
    fn test_finalize_rejects_unknown_hash() {
        let mut fc = ForkChoice::new();
        // Try to finalize a hash that was never added
        assert!(!fc.finalize(1, hash(99)));
        assert!(!fc.is_finalized(1));
    }

    #[test]
    fn test_competing_blocks_deterministic() {
        let mut fc = ForkChoice::new();

        let high_hash = hash(0xFF); // higher hash value
        let low_hash = hash(0x01); // lower hash value

        // Add the higher hash first
        fc.add_block(5, high_hash);
        assert_eq!(fc.canonical_block(5), Some(high_hash));

        // Now add the lower hash — it should win the tiebreak
        let is_fork = fc.add_block(5, low_hash);
        assert!(is_fork, "second block at same slot should be a fork");
        assert_eq!(
            fc.canonical_block(5),
            Some(low_hash),
            "canonical block should be the one with the lower hash, not first-seen"
        );
    }

    #[test]
    fn test_prune_removes_old_slots() {
        let mut fc = ForkChoice::new();
        fc.add_block(1, hash(1));
        fc.add_block(2, hash(2));
        fc.add_block(5, hash(5));
        fc.finalize(1, hash(1));

        fc.prune_before(3);

        assert_eq!(fc.canonical_block(1), None);
        assert_eq!(fc.canonical_block(2), None);
        assert_eq!(fc.canonical_block(5), Some(hash(5)));
        assert!(!fc.is_finalized(1));
    }

    #[test]
    fn test_three_way_fork_lowest_hash_wins() {
        let mut fc = ForkChoice::new();
        fc.add_block(10, hash(0xCC));
        fc.add_block(10, hash(0xAA));
        fc.add_block(10, hash(0xBB));

        assert_eq!(fc.canonical_block(10), Some(hash(0xAA)));
        assert_eq!(fc.candidates_for(10).len(), 3);
    }

    #[test]
    fn test_finalize_overrides_lower_hash_tiebreak() {
        let mut fc = ForkChoice::new();
        fc.add_block(7, hash(0x01));
        fc.add_block(7, hash(0xFF));

        assert_eq!(fc.canonical_block(7), Some(hash(0x01)));

        // Finalize the higher hash (e.g., it got 2/3 votes)
        assert!(fc.finalize(7, hash(0xFF)));
        assert_eq!(
            fc.canonical_block(7),
            Some(hash(0xFF)),
            "finalized block should override tiebreak"
        );
    }

    #[test]
    fn test_per_slot_candidate_cap_prevents_oom() {
        let mut fc = ForkChoice::new();
        // Fill up the candidate cap for slot 1
        for i in 0..MAX_CANDIDATES_PER_SLOT {
            let h = hash(i as u8);
            fc.add_block(1, h);
        }
        assert_eq!(fc.candidates_for(1).len(), MAX_CANDIDATES_PER_SLOT);

        // One more should be rejected
        let extra = hash(0xFE);
        let is_fork = fc.add_block(1, extra);
        assert!(!is_fork, "excess candidate should be rejected");
        assert_eq!(fc.candidates_for(1).len(), MAX_CANDIDATES_PER_SLOT);

        // Different slot should still work
        assert!(!fc.add_block(2, hash(0x01)));
        assert_eq!(fc.candidates_for(2).len(), 1);
    }

    #[test]
    fn test_post_finalization_block_rejected() {
        let mut fc = ForkChoice::new();
        fc.add_block(3, hash(0x01));
        fc.finalize(3, hash(0x01));

        // Late-arriving fork for finalized slot
        let is_fork = fc.add_block(3, hash(0x02));
        assert!(!is_fork, "blocks at finalized slots should be rejected");
        assert_eq!(fc.candidates_for(3).len(), 1);
        assert_eq!(fc.canonical_block(3), Some(hash(0x01)));
    }

    #[test]
    fn test_committed_slot_rejects_late_fork() {
        let mut fc = ForkChoice::new();
        // Block arrives and state is committed
        fc.add_block(5, hash(0xFF));
        fc.mark_committed(5);

        // Late-arriving fork with lower hash would normally win tiebreak,
        // but must be rejected because state is already committed
        let is_fork = fc.add_block(5, hash(0x01));
        assert!(!is_fork, "blocks at committed slots should be rejected");
        assert_eq!(fc.candidates_for(5).len(), 1);
        assert_eq!(
            fc.canonical_block(5),
            Some(hash(0xFF)),
            "committed canonical block must not change"
        );
    }

    #[test]
    fn test_uncommitted_slot_allows_tiebreak() {
        let mut fc = ForkChoice::new();
        // Block arrives but state is NOT committed yet
        fc.add_block(5, hash(0xFF));
        // Another block arrives before commit — tiebreak should work
        let is_fork = fc.add_block(5, hash(0x01));
        assert!(is_fork);
        assert_eq!(
            fc.canonical_block(5),
            Some(hash(0x01)),
            "lower hash should win when slot is not yet committed"
        );
    }

    // ---- Property-based tests ----

    mod proptests {
        use super::*;
        use proptest::collection::vec as pvec;
        use proptest::prelude::*;

        fn arb_hash() -> impl Strategy<Value = H256> {
            any::<[u8; 32]>().prop_map(|b| H256::from_slice(&b).unwrap())
        }

        /// After any sequence of add_block calls, canonical_block always returns
        /// the block with the lexicographically smallest hash for that slot.
        #[test]
        fn canonical_is_always_lowest_hash() {
            proptest!(|(
                slot in 0u64..100,
                hashes in pvec(arb_hash(), 1..=MAX_CANDIDATES_PER_SLOT)
            )| {
                let mut fc = ForkChoice::new();
                for h in &hashes {
                    fc.add_block(slot, *h);
                }
                let expected_min = hashes.iter().min_by_key(|h| h.as_bytes().to_vec()).unwrap();
                prop_assert_eq!(fc.canonical_block(slot), Some(*expected_min));
            });
        }

        /// Finalized blocks always take precedence over the hash-tiebreak rule.
        #[test]
        fn finalized_overrides_tiebreak() {
            proptest!(|(
                slot in 0u64..100,
                hashes in pvec(arb_hash(), 2..=8)
            )| {
                let mut fc = ForkChoice::new();
                for h in &hashes {
                    fc.add_block(slot, *h);
                }
                // Finalize the LAST hash (likely not the min)
                let target = *hashes.last().unwrap();
                fc.finalize(slot, target);
                prop_assert_eq!(fc.canonical_block(slot), Some(target));
                prop_assert!(fc.is_finalized(slot));
            });
        }

        /// No blocks can be added to finalized slots.
        #[test]
        fn finalized_slot_rejects_new_blocks() {
            proptest!(|(
                slot in 0u64..100,
                initial in arb_hash(),
                extras in pvec(arb_hash(), 1..=5)
            )| {
                let mut fc = ForkChoice::new();
                fc.add_block(slot, initial);
                fc.finalize(slot, initial);
                for h in &extras {
                    let is_fork = fc.add_block(slot, *h);
                    prop_assert!(!is_fork, "should reject block at finalized slot");
                }
                prop_assert_eq!(fc.candidates_for(slot).len(), 1);
            });
        }

        /// No blocks can be added to committed slots.
        #[test]
        fn committed_slot_rejects_new_blocks() {
            proptest!(|(
                slot in 0u64..100,
                initial in arb_hash(),
                extras in pvec(arb_hash(), 1..=5)
            )| {
                let mut fc = ForkChoice::new();
                fc.add_block(slot, initial);
                fc.mark_committed(slot);
                for h in &extras {
                    let is_fork = fc.add_block(slot, *h);
                    prop_assert!(!is_fork);
                }
                prop_assert_eq!(fc.canonical_block(slot), Some(initial));
            });
        }

        /// Candidate count never exceeds MAX_CANDIDATES_PER_SLOT.
        #[test]
        fn candidate_count_bounded() {
            proptest!(|(
                slot in 0u64..100,
                hashes in pvec(arb_hash(), 1..=32)
            )| {
                let mut fc = ForkChoice::new();
                for h in &hashes {
                    fc.add_block(slot, *h);
                }
                prop_assert!(fc.candidates_for(slot).len() <= MAX_CANDIDATES_PER_SLOT);
            });
        }

        /// prune_before removes exactly the slots below the threshold.
        #[test]
        fn prune_removes_only_old_slots() {
            proptest!(|(
                slots in pvec(0u64..200, 1..=20),
                threshold in 0u64..200
            )| {
                let mut fc = ForkChoice::new();
                let hash0 = H256::from_slice(&[0x01; 32]).unwrap();
                for &s in &slots {
                    fc.add_block(s, hash0);
                }
                fc.prune_before(threshold);
                for &s in &slots {
                    if s < threshold {
                        prop_assert_eq!(fc.canonical_block(s), None);
                    }
                    // slots >= threshold that were added should still exist
                    if s >= threshold {
                        prop_assert_eq!(fc.canonical_block(s), Some(hash0));
                    }
                }
            });
        }

        /// Duplicate adds are idempotent — candidate count stays the same.
        #[test]
        fn duplicate_add_idempotent() {
            proptest!(|(
                slot in 0u64..100,
                h in arb_hash(),
                repeats in 1usize..=10
            )| {
                let mut fc = ForkChoice::new();
                for _ in 0..repeats {
                    fc.add_block(slot, h);
                }
                prop_assert_eq!(fc.candidates_for(slot).len(), 1);
                prop_assert_eq!(fc.canonical_block(slot), Some(h));
            });
        }
    }

    #[test]
    fn test_prune_clears_committed() {
        let mut fc = ForkChoice::new();
        fc.add_block(1, hash(0x01));
        fc.mark_committed(1);
        fc.add_block(5, hash(0x05));
        fc.mark_committed(5);

        fc.prune_before(3);
        // Slot 1 committed status should be pruned
        // Slot 5 should remain committed
        assert_eq!(fc.canonical_block(1), None);
        assert_eq!(fc.canonical_block(5), Some(hash(0x05)));
    }
}
