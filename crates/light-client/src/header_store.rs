use aether_types::{BlockHeader, Slot, H256};
use std::collections::BTreeMap;

/// Stores verified finalized headers for the light client.
///
/// Only keeps the last `max_headers` to bound memory usage.
pub struct HeaderStore {
    headers: BTreeMap<Slot, BlockHeader>,
    max_headers: usize,
}

impl HeaderStore {
    pub fn new(max_headers: usize) -> Self {
        HeaderStore {
            headers: BTreeMap::new(),
            max_headers,
        }
    }

    /// Store a verified header.
    pub fn insert(&mut self, header: BlockHeader) {
        self.headers.insert(header.slot, header);

        // Evict oldest if over capacity
        while self.headers.len() > self.max_headers {
            self.headers.pop_first();
        }
    }

    /// Get a header by slot.
    pub fn get(&self, slot: Slot) -> Option<&BlockHeader> {
        self.headers.get(&slot)
    }

    /// Get the latest stored header.
    pub fn latest(&self) -> Option<&BlockHeader> {
        self.headers.values().next_back()
    }

    /// Get the state root at a given slot.
    pub fn state_root_at(&self, slot: Slot) -> Option<H256> {
        self.headers.get(&slot).map(|h| h.state_root)
    }

    pub fn len(&self) -> usize {
        self.headers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::*;

    fn make_header(slot: u64) -> BlockHeader {
        BlockHeader {
            version: 1,
            slot,
            parent_hash: H256::zero(),
            state_root: H256::from_slice(&[slot as u8; 32]).unwrap(),
            transactions_root: H256::zero(),
            receipts_root: H256::zero(),
            proposer: Address::from_slice(&[1u8; 20]).unwrap(),
            vrf_proof: VrfProof {
                output: [0u8; 32],
                proof: vec![0u8; 80],
            },
            timestamp: 1000 + slot,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let mut store = HeaderStore::new(100);
        store.insert(make_header(5));
        assert!(store.get(5).is_some());
        assert!(store.get(6).is_none());
    }

    #[test]
    fn test_latest() {
        let mut store = HeaderStore::new(100);
        store.insert(make_header(3));
        store.insert(make_header(7));
        store.insert(make_header(5));
        assert_eq!(store.latest().unwrap().slot, 7);
    }

    #[test]
    fn test_eviction() {
        let mut store = HeaderStore::new(3);
        for i in 0..5 {
            store.insert(make_header(i));
        }
        assert_eq!(store.len(), 3);
        // Oldest (0, 1) should be evicted
        assert!(store.get(0).is_none());
        assert!(store.get(1).is_none());
        assert!(store.get(2).is_some());
    }

    #[test]
    fn test_state_root_at() {
        let mut store = HeaderStore::new(100);
        store.insert(make_header(10));
        let root = store.state_root_at(10).unwrap();
        assert_ne!(root, H256::zero());
    }
}
