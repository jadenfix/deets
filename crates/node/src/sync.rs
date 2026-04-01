use aether_types::{Block, Slot};

/// Sync state machine for catching up to the network tip.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState {
    /// Fully synced with the network.
    Synced,
    /// Currently syncing from `from_slot` to `target_slot`.
    Syncing { from_slot: Slot, target_slot: Slot },
    /// Sync stalled (no progress for too long).
    Stalled,
}

/// Manages block synchronization for nodes that are behind the network.
pub struct SyncManager {
    state: SyncState,
    /// How many slots behind before we consider ourselves "out of sync".
    sync_threshold: u64,
    /// Blocks received during sync (buffered for ordered processing).
    sync_buffer: Vec<Block>,
}

impl SyncManager {
    pub fn new(sync_threshold: u64) -> Self {
        SyncManager {
            state: SyncState::Synced,
            sync_threshold,
            sync_buffer: Vec::new(),
        }
    }

    /// Check if we need to sync based on our latest slot vs network slot.
    pub fn check_sync_needed(&mut self, my_latest_slot: Slot, network_slot: Slot) -> bool {
        if network_slot > my_latest_slot + self.sync_threshold {
            self.state = SyncState::Syncing {
                from_slot: my_latest_slot,
                target_slot: network_slot,
            };
            true
        } else {
            if matches!(self.state, SyncState::Syncing { .. }) {
                self.state = SyncState::Synced;
            }
            false
        }
    }

    /// Get the current sync state.
    pub fn state(&self) -> &SyncState {
        &self.state
    }

    /// Check if we're currently syncing.
    pub fn is_syncing(&self) -> bool {
        matches!(self.state, SyncState::Syncing { .. })
    }

    /// Buffer a block received during sync.
    pub fn buffer_block(&mut self, block: Block) {
        self.sync_buffer.push(block);
    }

    /// Drain buffered blocks sorted by slot for ordered processing.
    pub fn drain_buffered(&mut self) -> Vec<Block> {
        self.sync_buffer.sort_by_key(|b| b.header.slot);
        std::mem::take(&mut self.sync_buffer)
    }

    /// Mark sync as complete.
    pub fn mark_synced(&mut self) {
        self.state = SyncState::Synced;
        self.sync_buffer.clear();
    }

    /// Get the range of slots we need to sync.
    pub fn sync_range(&self) -> Option<(Slot, Slot)> {
        match self.state {
            SyncState::Syncing {
                from_slot,
                target_slot,
            } => Some((from_slot, target_slot)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_not_needed_when_close() {
        let mut sync = SyncManager::new(10);
        assert!(!sync.check_sync_needed(95, 100));
        assert_eq!(sync.state(), &SyncState::Synced);
    }

    #[test]
    fn test_sync_needed_when_far_behind() {
        let mut sync = SyncManager::new(10);
        assert!(sync.check_sync_needed(50, 100));
        assert!(sync.is_syncing());
        assert_eq!(sync.sync_range(), Some((50, 100)));
    }

    #[test]
    fn test_sync_completes_when_caught_up() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(50, 100);
        assert!(sync.is_syncing());

        // Now we've caught up
        sync.check_sync_needed(95, 100);
        assert!(!sync.is_syncing());
    }

    #[test]
    fn test_buffer_and_drain() {
        let mut sync = SyncManager::new(10);

        let block5 = aether_types::Block::new(
            5,
            aether_types::H256::zero(),
            aether_types::Address::from_slice(&[1u8; 20]).unwrap(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );
        let block3 = aether_types::Block::new(
            3,
            aether_types::H256::zero(),
            aether_types::Address::from_slice(&[1u8; 20]).unwrap(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );

        sync.buffer_block(block5);
        sync.buffer_block(block3);

        let drained = sync.drain_buffered();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].header.slot, 3); // Sorted by slot
        assert_eq!(drained[1].header.slot, 5);
    }
}
