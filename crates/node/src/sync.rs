use aether_types::{Block, H256, Slot};
use std::time::{Duration, Instant};

/// Maximum blocks to buffer during sync to prevent OOM.
const MAX_SYNC_BUFFER: usize = 1024;

/// Maximum number of slots to request per sync batch.
const SYNC_BATCH_SIZE: u64 = 64;

/// After this duration of no progress, sync transitions to Stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

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
///
/// When a node detects it is behind the network tip by more than
/// `sync_threshold` slots, it enters syncing mode: it requests missing
/// blocks in batches via gossipsub, buffers responses, and applies them
/// in slot order once each batch is complete.
pub struct SyncManager {
    state: SyncState,
    /// How many slots behind before we consider ourselves "out of sync".
    sync_threshold: u64,
    /// Blocks received during sync (buffered for ordered processing).
    sync_buffer: Vec<Block>,
    /// Highest slot we have successfully applied so far.
    next_expected_slot: Slot,
    /// Parent hash expected for the next block (for chain continuity validation).
    expected_parent_hash: Option<H256>,
    /// Last time we made progress (received a useful block).
    last_progress: Option<Instant>,
    /// End of the current batch being requested.
    current_batch_end: Slot,
    /// Count of blocks successfully applied during this sync session.
    blocks_applied: u64,
}

impl SyncManager {
    pub fn new(sync_threshold: u64) -> Self {
        SyncManager {
            state: SyncState::Synced,
            sync_threshold,
            sync_buffer: Vec::new(),
            next_expected_slot: 0,
            expected_parent_hash: None,
            last_progress: None,
            current_batch_end: 0,
            blocks_applied: 0,
        }
    }

    /// Check if we need to sync based on our latest slot vs network slot.
    pub fn check_sync_needed(&mut self, my_latest_slot: Slot, network_slot: Slot) -> bool {
        if network_slot > my_latest_slot + self.sync_threshold {
            if !self.is_syncing() {
                self.next_expected_slot = my_latest_slot + 1;
                self.current_batch_end = 0;
                self.last_progress = Some(Instant::now());
                self.blocks_applied = 0;
            }
            self.state = SyncState::Syncing {
                from_slot: my_latest_slot,
                target_slot: network_slot,
            };
            true
        } else {
            if matches!(self.state, SyncState::Syncing { .. }) {
                self.mark_synced();
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

    /// Buffer a block received during sync. Returns false if buffer is full.
    pub fn buffer_block(&mut self, block: Block) -> bool {
        if self.sync_buffer.len() >= MAX_SYNC_BUFFER {
            return false;
        }
        // Track progress when we receive a block we need.
        self.last_progress = Some(Instant::now());
        self.sync_buffer.push(block);
        true
    }

    /// Drain buffered blocks that form a contiguous sequence starting at
    /// `next_expected_slot`. Returns them sorted by slot, validated for
    /// parent hash chain continuity. Blocks that don't continue the chain
    /// or fail parent hash checks are kept in the buffer.
    pub fn drain_ready(&mut self) -> Vec<Block> {
        if self.sync_buffer.is_empty() {
            return Vec::new();
        }
        self.sync_buffer.sort_by_key(|b| b.header.slot);

        let mut ready = Vec::new();
        let mut remaining = Vec::new();

        for block in self.sync_buffer.drain(..) {
            if block.header.slot == self.next_expected_slot {
                // Validate parent hash chain continuity when we have an expected parent.
                if let Some(expected) = self.expected_parent_hash {
                    if block.header.parent_hash != expected
                        && block.header.parent_hash != H256::zero()
                    {
                        // Parent mismatch — keep in buffer (might be from a different fork).
                        remaining.push(block);
                        continue;
                    }
                }
                self.expected_parent_hash = Some(block.hash());
                self.next_expected_slot = self.next_expected_slot.saturating_add(1);
                ready.push(block);
            } else if block.header.slot > self.next_expected_slot {
                remaining.push(block);
            }
            // blocks with slot < next_expected_slot are duplicates, drop them
        }

        // Continue draining any that now form a contiguous sequence
        remaining.sort_by_key(|b| b.header.slot);
        while !remaining.is_empty() && remaining[0].header.slot == self.next_expected_slot {
            let block = &remaining[0];
            // Validate parent hash chain for continuation blocks too.
            if let Some(expected) = self.expected_parent_hash {
                if block.header.parent_hash != expected
                    && block.header.parent_hash != H256::zero()
                {
                    break;
                }
            }
            let block = remaining.remove(0);
            self.expected_parent_hash = Some(block.hash());
            self.next_expected_slot = self.next_expected_slot.saturating_add(1);
            ready.push(block);
        }

        self.sync_buffer = remaining;
        ready
    }

    /// Get the next batch of slots to request from peers.
    /// Returns `None` if we're not syncing or waiting for the current batch.
    pub fn next_request(&mut self) -> Option<(Slot, Slot)> {
        let target = match self.state {
            SyncState::Syncing { target_slot, .. } => target_slot,
            _ => return None,
        };

        // Don't request if we're still waiting for the current batch
        if self.current_batch_end >= self.next_expected_slot
            && !self.sync_buffer.is_empty()
        {
            return None;
        }

        let from = self.next_expected_slot;
        if from > target {
            return None;
        }

        let to = (from + SYNC_BATCH_SIZE).min(target);
        self.current_batch_end = to;
        Some((from, to))
    }

    /// Check if sync has stalled (no progress for STALL_TIMEOUT).
    pub fn check_stalled(&mut self) -> bool {
        if let Some(last) = self.last_progress {
            if last.elapsed() >= STALL_TIMEOUT {
                self.state = SyncState::Stalled;
                return true;
            }
        }
        false
    }

    /// Reset stall state and retry from where we left off.
    pub fn retry_after_stall(&mut self, target_slot: Slot) {
        self.state = SyncState::Syncing {
            from_slot: self.next_expected_slot,
            target_slot,
        };
        self.current_batch_end = 0;
        self.last_progress = Some(Instant::now());
    }

    /// Mark sync as complete.
    pub fn mark_synced(&mut self) {
        self.state = SyncState::Synced;
        self.sync_buffer.clear();
        self.current_batch_end = 0;
        self.expected_parent_hash = None;
        self.blocks_applied = 0;
    }

    /// Set the expected parent hash for chain continuity validation.
    /// Should be called with the hash of the last locally-applied block
    /// before sync begins.
    pub fn set_expected_parent(&mut self, hash: H256) {
        self.expected_parent_hash = Some(hash);
    }

    /// Record that a synced block was successfully applied.
    pub fn record_applied(&mut self) {
        self.blocks_applied += 1;
    }

    /// Get the number of blocks applied during this sync session.
    pub fn blocks_applied(&self) -> u64 {
        self.blocks_applied
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

    /// Number of blocks currently buffered.
    pub fn buffer_len(&self) -> usize {
        self.sync_buffer.len()
    }

    /// The next slot we expect to receive.
    pub fn next_expected(&self) -> Slot {
        self.next_expected_slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(slot: Slot) -> Block {
        Block::new(
            slot,
            H256::zero(),
            aether_types::Address::from_slice(&[1u8; 20]).unwrap(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        )
    }

    /// Create a chain of blocks with valid parent hash links.
    fn make_chain(start_slot: Slot, count: usize, parent: H256) -> Vec<Block> {
        let mut blocks = Vec::with_capacity(count);
        let mut prev_hash = parent;
        for i in 0..count {
            let block = Block::new(
                start_slot + i as u64,
                prev_hash,
                aether_types::Address::from_slice(&[1u8; 20]).unwrap(),
                aether_types::VrfProof {
                    output: [0u8; 32],
                    proof: vec![],
                },
                vec![],
            );
            prev_hash = block.hash();
            blocks.push(block);
        }
        blocks
    }

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
        assert_eq!(sync.next_expected(), 51);
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
    fn test_buffer_bounded() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 2000);

        for i in 0..MAX_SYNC_BUFFER {
            assert!(sync.buffer_block(make_block(i as u64 + 1)));
        }
        // Buffer full — should reject
        assert!(!sync.buffer_block(make_block(MAX_SYNC_BUFFER as u64 + 1)));
        assert_eq!(sync.buffer_len(), MAX_SYNC_BUFFER);
    }

    #[test]
    fn test_drain_ready_contiguous() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);
        // next_expected = 1

        // Buffer slots 1, 2, 3 out of order
        sync.buffer_block(make_block(3));
        sync.buffer_block(make_block(1));
        sync.buffer_block(make_block(2));

        let ready = sync.drain_ready();
        assert_eq!(ready.len(), 3);
        assert_eq!(ready[0].header.slot, 1);
        assert_eq!(ready[1].header.slot, 2);
        assert_eq!(ready[2].header.slot, 3);
        assert_eq!(sync.next_expected(), 4);
    }

    #[test]
    fn test_drain_ready_with_gap() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);
        // next_expected = 1

        // Buffer slots 1, 2, 5 (gap at 3,4)
        sync.buffer_block(make_block(1));
        sync.buffer_block(make_block(2));
        sync.buffer_block(make_block(5));

        let ready = sync.drain_ready();
        assert_eq!(ready.len(), 2); // only 1,2
        assert_eq!(ready[0].header.slot, 1);
        assert_eq!(ready[1].header.slot, 2);
        assert_eq!(sync.next_expected(), 3);
        // slot 5 is still buffered
        assert_eq!(sync.buffer_len(), 1);
    }

    #[test]
    fn test_drain_drops_duplicates() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(5, 100);
        // next_expected = 6

        // Buffer a block that's already been applied (slot 3)
        sync.buffer_block(make_block(3));
        sync.buffer_block(make_block(6));

        let ready = sync.drain_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].header.slot, 6);
        assert_eq!(sync.buffer_len(), 0); // slot 3 was dropped
    }

    #[test]
    fn test_next_request_batch() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 200);

        let req = sync.next_request();
        assert_eq!(req, Some((1, 1 + SYNC_BATCH_SIZE)));
    }

    #[test]
    fn test_next_request_none_when_synced() {
        let mut sync = SyncManager::new(10);
        assert!(sync.next_request().is_none());
    }

    #[test]
    fn test_next_request_capped_at_target() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 20);

        let req = sync.next_request();
        assert_eq!(req, Some((1, 20)));
    }

    #[test]
    fn test_stall_detection() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);
        // Immediately after check, not stalled
        assert!(!sync.check_stalled());

        // Force stall by backdating last_progress
        sync.last_progress = Some(Instant::now() - STALL_TIMEOUT - Duration::from_secs(1));
        assert!(sync.check_stalled());
        assert_eq!(sync.state(), &SyncState::Stalled);
    }

    #[test]
    fn test_retry_after_stall() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);
        sync.next_expected_slot = 30;

        sync.last_progress = Some(Instant::now() - STALL_TIMEOUT - Duration::from_secs(1));
        sync.check_stalled();
        assert_eq!(sync.state(), &SyncState::Stalled);

        sync.retry_after_stall(100);
        assert!(sync.is_syncing());
        assert_eq!(sync.sync_range(), Some((30, 100)));
    }

    #[test]
    fn test_mark_synced_clears_state() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);
        sync.buffer_block(make_block(1));
        assert_eq!(sync.buffer_len(), 1);

        sync.mark_synced();
        assert_eq!(sync.state(), &SyncState::Synced);
        assert_eq!(sync.buffer_len(), 0);
    }

    #[test]
    fn test_drain_validates_parent_hash_chain() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);
        // next_expected = 1

        // Create a valid chain: genesis -> block1 -> block2 -> block3
        let genesis_hash = H256::zero();
        let chain = make_chain(1, 3, genesis_hash);

        // Set expected parent to genesis
        sync.set_expected_parent(genesis_hash);

        // Buffer in reverse order
        sync.buffer_block(chain[2].clone());
        sync.buffer_block(chain[0].clone());
        sync.buffer_block(chain[1].clone());

        let ready = sync.drain_ready();
        assert_eq!(ready.len(), 3);
        assert_eq!(ready[0].header.slot, 1);
        assert_eq!(ready[1].header.slot, 2);
        assert_eq!(ready[2].header.slot, 3);
        // Parent hash chain is valid
        assert_eq!(ready[0].header.parent_hash, genesis_hash);
        assert_eq!(ready[1].header.parent_hash, ready[0].hash());
        assert_eq!(ready[2].header.parent_hash, ready[1].hash());
    }

    #[test]
    fn test_drain_rejects_wrong_parent_hash() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);

        let genesis_hash = H256::zero();
        sync.set_expected_parent(genesis_hash);

        // Create a valid chain from genesis
        let chain = make_chain(1, 1, genesis_hash);
        // Create a block at slot 2 with a wrong parent (not chain[0].hash())
        let wrong_parent = Block::new(
            2,
            H256::from_slice(&[0xAA; 32]).unwrap(),
            aether_types::Address::from_slice(&[1u8; 20]).unwrap(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );

        sync.buffer_block(chain[0].clone());
        sync.buffer_block(wrong_parent);

        let ready = sync.drain_ready();
        // Only the first block should be drained; slot 2 has wrong parent
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].header.slot, 1);
        // The wrong-parent block stays in the buffer
        assert_eq!(sync.buffer_len(), 1);
    }

    #[test]
    fn test_blocks_applied_tracking() {
        let mut sync = SyncManager::new(10);
        assert_eq!(sync.blocks_applied(), 0);

        sync.check_sync_needed(0, 100);
        assert_eq!(sync.blocks_applied(), 0);

        sync.record_applied();
        sync.record_applied();
        assert_eq!(sync.blocks_applied(), 2);

        sync.mark_synced();
        assert_eq!(sync.blocks_applied(), 0);
    }

    #[test]
    fn test_drain_chain_out_of_order_with_parent_validation() {
        let mut sync = SyncManager::new(10);
        sync.check_sync_needed(0, 100);

        let genesis_hash = H256::zero();
        let chain = make_chain(1, 5, genesis_hash);
        sync.set_expected_parent(genesis_hash);

        // Buffer slots 1,2,5 (gap at 3,4)
        sync.buffer_block(chain[4].clone()); // slot 5
        sync.buffer_block(chain[1].clone()); // slot 2
        sync.buffer_block(chain[0].clone()); // slot 1

        let ready = sync.drain_ready();
        assert_eq!(ready.len(), 2); // only 1,2 (contiguous)
        assert_eq!(sync.next_expected(), 3);
        assert_eq!(sync.buffer_len(), 1); // slot 5 still buffered

        // Now buffer 3,4 to fill the gap
        sync.buffer_block(chain[2].clone()); // slot 3
        sync.buffer_block(chain[3].clone()); // slot 4

        let ready = sync.drain_ready();
        assert_eq!(ready.len(), 3); // 3,4,5
        assert_eq!(sync.next_expected(), 6);
        assert_eq!(sync.buffer_len(), 0);
    }
}
