use aether_types::{Slot, H256};
use std::collections::HashMap;

/// Simple fork choice: for each slot, track all candidate blocks and
/// select the canonical one based on finality or first-seen ordering.
#[derive(Default)]
pub struct ForkChoice {
    /// All known blocks per slot (can have multiple competing blocks).
    candidates: HashMap<Slot, Vec<H256>>,
    /// The chosen canonical block per slot.
    canonical: HashMap<Slot, H256>,
    /// Finalized blocks (immutable once set).
    finalized: HashMap<Slot, H256>,
}

impl ForkChoice {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new block candidate for a slot. Returns true if this is a new fork
    /// (competing block for an already-occupied slot).
    pub fn add_block(&mut self, slot: Slot, block_hash: H256) -> bool {
        // Reject new blocks for finalized slots — finalization is irreversible
        if self.finalized.contains_key(&slot) {
            return false;
        }

        let candidates = self.candidates.entry(slot).or_default();
        if candidates.contains(&block_hash) {
            return false; // Already known
        }
        let is_fork = !candidates.is_empty();
        candidates.push(block_hash);

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

    /// Prune data for slots before `min_slot` to bound memory.
    pub fn prune_before(&mut self, min_slot: Slot) {
        self.candidates.retain(|&s, _| s >= min_slot);
        self.canonical.retain(|&s, _| s >= min_slot);
        self.finalized.retain(|&s, _| s >= min_slot);
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
}
