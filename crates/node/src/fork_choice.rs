use aether_types::{Slot, H256};
use std::collections::HashMap;

/// Simple fork choice: for each slot, track all candidate blocks and
/// select the canonical one based on finality or first-seen ordering.
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
        ForkChoice {
            candidates: HashMap::new(),
            canonical: HashMap::new(),
            finalized: HashMap::new(),
        }
    }

    /// Record a new block candidate for a slot. Returns true if this is a new fork
    /// (competing block for an already-occupied slot).
    pub fn add_block(&mut self, slot: Slot, block_hash: H256) -> bool {
        let candidates = self.candidates.entry(slot).or_default();
        if candidates.contains(&block_hash) {
            return false; // Already known
        }
        let is_fork = !candidates.is_empty();
        candidates.push(block_hash);

        // If finalized, canonical doesn't change
        if self.finalized.contains_key(&slot) {
            return is_fork;
        }

        // First block or override: set canonical to first-seen
        if !self.canonical.contains_key(&slot) {
            self.canonical.insert(slot, block_hash);
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

    /// Finalize a block at a slot. Once finalized, the canonical choice is permanent.
    pub fn finalize(&mut self, slot: Slot, block_hash: H256) {
        self.finalized.insert(slot, block_hash);
        self.canonical.insert(slot, block_hash);
    }

    /// Check if a slot has competing blocks (a fork).
    pub fn has_fork(&self, slot: Slot) -> bool {
        self.candidates
            .get(&slot)
            .map_or(false, |c| c.len() > 1)
    }

    /// Get all candidate blocks for a slot.
    pub fn candidates_for(&self, slot: Slot) -> &[H256] {
        self.candidates
            .get(&slot)
            .map_or(&[], |v| v.as_slice())
    }

    /// Check if a slot is finalized.
    pub fn is_finalized(&self, slot: Slot) -> bool {
        self.finalized.contains_key(&slot)
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
        // First-seen wins
        assert_eq!(fc.canonical_block(1), Some(hash(1)));
    }

    #[test]
    fn finalized_block_cannot_be_overridden() {
        let mut fc = ForkChoice::new();
        fc.add_block(1, hash(1));
        fc.finalize(1, hash(1));

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
}
