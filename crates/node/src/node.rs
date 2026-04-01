use aether_consensus::slashing::{self as slash_verify, SlashProof, SlashType, Vote as SlashVote};
use aether_consensus::{ConsensusEngine, SlashingDetector};
use aether_crypto_bls::BlsKeypair;
use aether_crypto_primitives::Keypair;
use aether_ledger::{EmissionSchedule, FeeMarket, Ledger};
use aether_mempool::Mempool;
use aether_p2p::network::NetworkEvent;
use aether_program_staking::StakingState;
use aether_state_storage::{
    database::pruning, Storage, StorageBatch, CF_BLOCKS, CF_METADATA, CF_RECEIPTS,
};
use aether_types::{
    Account, Address, Block, ChainConfig, PublicKey, Slot, Transaction, TransactionReceipt, Vote,
    H256,
};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time;

use crate::fork_choice::ForkChoice;
use crate::network_handler::{decode_network_event, NodeMessage, OutboundMessage};
use crate::poh::{PohMetrics, PohRecorder};
use crate::sync::SyncManager;

const MAX_OUTBOUND_BUFFER: usize = 10_000;
const MAX_CACHED_BLOCKS: usize = 10_000;
const MAX_CACHED_RECEIPTS: usize = 50_000;
/// Maximum number of orphan blocks to buffer while waiting for parents.
const MAX_ORPHAN_BLOCKS: usize = 256;

type LoadedBlocks = (
    BTreeMap<Slot, H256>,
    HashMap<H256, Block>,
    H256,
    Option<Slot>,
);

pub struct Node {
    chain_config: Arc<ChainConfig>,
    ledger: Ledger,
    mempool: Mempool,
    consensus: Box<dyn ConsensusEngine>,
    validator_key: Option<Keypair>,
    bls_key: Option<BlsKeypair>,
    running: bool,
    poh: PohRecorder,
    last_poh_metrics: Option<PohMetrics>,
    fee_market: FeeMarket,
    emission_schedule: EmissionSchedule,
    current_epoch: u64,
    fork_choice: ForkChoice,
    latest_block_hash: H256,
    latest_block_slot: Option<Slot>,
    blocks_by_slot: BTreeMap<Slot, H256>,
    blocks_by_hash: HashMap<H256, Block>,
    receipts: HashMap<H256, TransactionReceipt>,
    /// In-memory staking state for tracking validator stakes and applying slashes.
    staking_state: StakingState,
    /// Channel to send outbound messages (blocks, votes, txs) to P2P layer.
    broadcast_tx: Option<mpsc::Sender<OutboundMessage>>,
    /// Collected outbound messages when no broadcast channel is set (for testing).
    outbound_buffer: Vec<OutboundMessage>,
    /// Consecutive timeout counter for circuit breaker.
    consecutive_timeouts: u32,
    /// Detects double-signing and other slashable offenses from incoming votes.
    slashing_detector: SlashingDetector,
    /// Tracks sync state (synced, syncing, stalled).
    sync_manager: SyncManager,
    /// Orphan blocks waiting for their parent to arrive, keyed by parent hash.
    orphan_blocks: HashMap<H256, Vec<Block>>,
    /// Total number of orphan blocks buffered (across all parent hashes).
    orphan_count: usize,
}

impl Node {
    pub fn new<P: AsRef<Path>>(
        db_path: P,
        consensus: Box<dyn ConsensusEngine>,
        validator_key: Option<Keypair>,
        bls_key: Option<BlsKeypair>,
        chain_config: Arc<ChainConfig>,
    ) -> Result<Self> {
        let storage = Storage::open(db_path).context("failed to open storage")?;
        let ledger = Ledger::new(storage).context("failed to initialize ledger")?;
        let mempool = Mempool::new(chain_config.fees.clone(), chain_config.chain.chain_id_numeric);

        // Warn on asymmetric key configuration
        if validator_key.is_some() != bls_key.is_some() {
            tracing::warn!(
                has_validator_key = validator_key.is_some(),
                has_bls_key = bls_key.is_some(),
                "Asymmetric key config — voting disabled"
            );
        }

        // Load persisted blocks from disk
        let (blocks_by_slot, blocks_by_hash, latest_block_hash, latest_block_slot) =
            Self::load_blocks_from_storage(ledger.storage())?;

        if !blocks_by_hash.is_empty() {
            tracing::info!(
                block_count = blocks_by_hash.len(),
                tip_slot = ?latest_block_slot,
                "Recovered blocks from disk"
            );
        }

        let fee_market = FeeMarket::new(
            chain_config.fees.a,
            chain_config.chain.block_bytes_max,
            chain_config.fees.min_base_fee,
        );
        let emission_schedule = EmissionSchedule::new(
            chain_config.tokens.swr_initial_supply,
            chain_config.chain.slot_ms,
            chain_config.chain.epoch_slots,
        );
        Ok(Node {
            chain_config,
            ledger,
            mempool,
            consensus,
            validator_key,
            bls_key,
            running: false,
            poh: PohRecorder::new(),
            last_poh_metrics: None,
            fee_market,
            emission_schedule,
            current_epoch: 0,
            fork_choice: ForkChoice::new(),
            latest_block_hash,
            latest_block_slot,
            blocks_by_slot,
            blocks_by_hash,
            receipts: HashMap::new(),
            staking_state: StakingState::new(),
            broadcast_tx: None,
            outbound_buffer: Vec::new(),
            consecutive_timeouts: 0,
            slashing_detector: SlashingDetector::new(),
            sync_manager: SyncManager::new(10),
            orphan_blocks: HashMap::new(),
            orphan_count: 0,
        })
    }

    /// Load persisted blocks from RocksDB on startup.
    ///
    /// Only keeps the most recent MAX_CACHED_BLOCKS to bound memory usage
    /// instead of loading the entire block history.
    fn load_blocks_from_storage(storage: &Storage) -> Result<LoadedBlocks> {
        // Collect all blocks, then keep only the most recent MAX_CACHED_BLOCKS
        let mut all: Vec<(Slot, H256, Block)> = Vec::new();
        for (_, value) in storage.iterator(CF_BLOCKS)? {
            if let Ok(block) = bincode::deserialize::<Block>(&value) {
                let hash = block.hash();
                let slot = block.header.slot;
                all.push((slot, hash, block));
            }
        }
        all.sort_unstable_by_key(|(slot, _, _)| *slot);

        // Trim to the most recent blocks
        let start = all.len().saturating_sub(MAX_CACHED_BLOCKS);
        let recent = &all[start..];

        let mut by_slot = BTreeMap::new();
        let mut by_hash = HashMap::new();
        let mut latest_hash = H256::zero();
        let mut latest_slot: Option<Slot> = None;

        for (slot, hash, block) in recent {
            by_slot.insert(*slot, *hash);
            by_hash.insert(*hash, block.clone());
            if latest_slot.map_or(true, |s| *slot > s) {
                latest_slot = Some(*slot);
                latest_hash = *hash;
            }
        }

        Ok((by_slot, by_hash, latest_hash, latest_slot))
    }

    /// Build a StorageBatch for block and receipt persistence (without writing).
    /// Callers combine this with the overlay batch for a single atomic commit.
    fn build_block_batch(
        &self,
        block: &Block,
        block_hash: H256,
        receipts: &[TransactionReceipt],
    ) -> Result<StorageBatch> {
        let mut batch = StorageBatch::new();

        // Block data
        let block_bytes = bincode::serialize(block)?;
        batch.put(CF_BLOCKS, block_hash.as_bytes().to_vec(), block_bytes);

        // Slot→hash index
        let slot_key = format!("slot:{}", block.header.slot);
        batch.put(
            CF_METADATA,
            slot_key.as_bytes().to_vec(),
            block_hash.as_bytes().to_vec(),
        );

        // Receipts
        for receipt in receipts {
            let receipt_bytes = bincode::serialize(receipt)?;
            batch.put(
                CF_RECEIPTS,
                receipt.tx_hash.as_bytes().to_vec(),
                receipt_bytes,
            );
        }

        Ok(batch)
    }

    /// Set the broadcast channel for outbound P2P messages.
    pub fn set_broadcast_tx(&mut self, tx: mpsc::Sender<OutboundMessage>) {
        self.broadcast_tx = Some(tx);
    }

    /// Drain collected outbound messages (for testing without P2P).
    pub fn drain_outbound(&mut self) -> Vec<OutboundMessage> {
        std::mem::take(&mut self.outbound_buffer)
    }

    fn broadcast(&mut self, msg: OutboundMessage) {
        if let Some(ref tx) = self.broadcast_tx {
            match tx.try_send(msg) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_msg)) => {
                    // Backpressure: P2P layer can't keep up, drop the message
                    tracing::warn!("P2P outbound channel full, dropping message");
                }
                Err(mpsc::error::TrySendError::Closed(msg)) => {
                    // Channel closed — fall back to buffer so message isn't lost
                    tracing::warn!("Broadcast channel closed");
                    if self.outbound_buffer.len() < MAX_OUTBOUND_BUFFER {
                        self.outbound_buffer.push(msg);
                    }
                }
            }
        } else if self.outbound_buffer.len() < MAX_OUTBOUND_BUFFER {
            self.outbound_buffer.push(msg);
        } else {
            tracing::error!("Outbound buffer full ({MAX_OUTBOUND_BUFFER}), dropping message");
        }
    }

    pub fn submit_transaction(&mut self, tx: Transaction) -> Result<H256> {
        let tx_hash = tx.hash();
        self.mempool.add_transaction(tx.clone())?;
        self.broadcast(OutboundMessage::BroadcastTransaction(tx));
        Ok(tx_hash)
    }

    pub async fn run(&mut self) -> Result<()> {
        self.running = true;

        tracing::info!(
            validator = self.validator_key.is_some(),
            starting_slot = self.consensus.current_slot(),
            "Node starting"
        );

        while self.running {
            self.tick()?;

            // Wait for slot duration (500ms)
            time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    pub fn tick(&mut self) -> Result<()> {
        self.process_slot()?;
        self.consensus.advance_slot();
        Ok(())
    }

    fn process_slot(&mut self) -> Result<()> {
        let slot = self.consensus.current_slot();
        let _span = tracing::info_span!("process_slot", slot).entered();

        // Update mempool with current slot for forced inclusion tracking
        self.mempool.set_current_slot(slot);

        // Check for epoch transition
        let epoch = slot / self.chain_config.chain.epoch_slots;
        if epoch > self.current_epoch {
            self.process_epoch_transition(epoch)?;
        }
        self.current_epoch = epoch;

        // Check pacemaker timeout — if no quorum reached, advance to prevent deadlock
        if self.consensus.is_timed_out() {
            self.consecutive_timeouts += 1;
            if self.consecutive_timeouts >= 100 {
                tracing::error!(
                    consecutive_timeouts = self.consecutive_timeouts,
                    slot,
                    "CIRCUIT BREAKER — possible network partition or all peers down"
                );
            }
            tracing::warn!(
                slot,
                consecutive = self.consecutive_timeouts,
                "Slot timeout — advancing via pacemaker"
            );
            self.consensus.on_timeout();
        } else {
            self.consecutive_timeouts = 0;
        }

        let metrics = self.poh.tick(Instant::now());
        self.last_poh_metrics = Some(metrics.clone());
        tracing::debug!(
            last_ms = metrics.last_duration_ms,
            avg_ms = format_args!("{:.1}", metrics.average_duration_ms),
            jitter_ms = format_args!("{:.1}", metrics.jitter_ms),
            "PoH tick"
        );

        if let Some(ref keypair) = self.validator_key {
            let pubkey = PublicKey::from_bytes(keypair.public_key());

            if self.consensus.is_leader(slot, &pubkey) {
                tracing::info!(slot, "Leader — producing block");
                self.produce_block(slot)?;
            } else {
                tracing::debug!(slot, "Not leader — waiting for block");
            }
        }

        // Check if any slot can be finalized
        self.check_finality();

        // Evict old cached blocks/receipts to bound memory (Fix 10)
        self.evict_old_cache();

        Ok(())
    }

    /// Process epoch transition: distribute staking rewards.
    fn process_epoch_transition(&mut self, new_epoch: u64) -> Result<()> {
        let _span = tracing::info_span!("epoch_transition", epoch = new_epoch).entered();

        let slot = self.consensus.current_slot();
        let total_supply = self.chain_config.tokens.swr_initial_supply;
        let emission = self.emission_schedule.epoch_emission(slot, total_supply);

        if emission == 0 {
            return Ok(());
        }

        let total_stake = self.consensus.total_stake();
        if total_stake == 0 {
            return Ok(());
        }

        tracing::info!(
            prev_epoch = new_epoch - 1,
            new_epoch,
            emission,
            "Distributing emission rewards"
        );

        // Credit emission proportionally to each validator based on stake.
        // In production, this would integrate with the staking program's reward pool.
        // For now, credit each known validator proportionally.
        if let Some(ref keypair) = self.validator_key {
            // Credit the local validator their proportional share
            let my_pubkey = PublicKey::from_bytes(keypair.public_key());
            let my_addr = my_pubkey.to_address();
            let my_stake = self.consensus.validator_stake(&my_addr);
            if my_stake > 0 {
                let my_share = emission.checked_mul(my_stake).map(|n| n / total_stake).unwrap_or(0);
                if my_share > 0 {
                    if let Err(e) = self.ledger.credit_account(&my_addr, my_share) {
                        tracing::warn!(err = %e, "Failed to credit emission reward");
                    }
                }
            }
        }

        // Complete unbonding: return tokens to delegators whose unbonding period
        // has elapsed. complete_unbonding() returns (address, amount) pairs.
        let completed = self.staking_state.complete_unbonding(slot);
        for (addr, amount) in completed {
            if let Err(e) = self.ledger.credit_account(&addr, amount) {
                tracing::warn!(?addr, err = %e, "Failed to credit unbonded tokens");
            } else {
                tracing::info!(?addr, amount, "Returned unbonded tokens");
            }
        }

        // Prune old blocks and receipts from disk to prevent unbounded DB growth.
        let retention = self.chain_config.chain.retention_epochs;
        if retention > 0 && new_epoch > retention {
            let prune_before_epoch = new_epoch - retention;
            let prune_before_slot = prune_before_epoch * self.chain_config.chain.epoch_slots;
            if let Err(e) = pruning::prune_old_blocks(self.ledger.storage(), prune_before_slot) {
                tracing::warn!(err = %e, "Block pruning failed");
            }
            if let Err(e) = pruning::prune_old_receipts(self.ledger.storage(), prune_before_slot) {
                tracing::warn!(err = %e, "Receipt pruning failed");
            }
            tracing::info!(
                new_epoch,
                prune_before_slot,
                "Pruned old blocks/receipts"
            );
        }

        Ok(())
    }

    /// Evict oldest cached blocks/receipts to keep memory bounded.
    fn evict_old_cache(&mut self) {
        // Evict blocks exceeding cache limit — O(log n) per eviction via BTreeMap
        while self.blocks_by_hash.len() > MAX_CACHED_BLOCKS {
            if let Some((&min_slot, &hash)) = self.blocks_by_slot.iter().next() {
                self.blocks_by_slot.remove(&min_slot);
                self.blocks_by_hash.remove(&hash);
            } else {
                break;
            }
        }

        // Evict oldest receipts by slot (not random HashMap order)
        if self.receipts.len() > MAX_CACHED_RECEIPTS {
            let mut by_slot: Vec<(u64, H256)> = self
                .receipts
                .iter()
                .map(|(hash, r)| (r.slot, *hash))
                .collect();
            by_slot.sort_unstable_by_key(|(slot, _)| *slot);
            for (_, hash) in by_slot.into_iter().take(1000) {
                self.receipts.remove(&hash);
            }
        }

        // Prune fork choice and slashing detector for finalized slots
        let finalized = self.consensus.finalized_slot();
        self.fork_choice.prune_before(finalized);
        self.slashing_detector.prune_before(finalized);
    }

    fn produce_block(&mut self, slot: Slot) -> Result<()> {
        let _span = tracing::info_span!("produce_block", slot).entered();

        // Forced inclusion: include txs that have been waiting too long (anti-censorship)
        let forced = self
            .mempool
            .must_include_transactions(slot, self.fee_market.base_fee);
        let forced_count = forced.len();
        let remaining_capacity = 1000usize.saturating_sub(forced_count);
        let regular = self.mempool.get_transactions(remaining_capacity, 5_000_000);
        let transactions = if forced_count > 0 {
            tracing::info!(forced_count, "Forced inclusion txs");
            let mut all = forced;
            all.extend(regular);
            all
        } else {
            regular
        };

        tracing::info!(tx_count = transactions.len(), "Including transactions");

        // Get VRF proof FIRST (before execution, to fail fast if not leader)
        let vrf_proof_crypto = self.consensus.get_leader_proof(slot);
        let vrf_proof = if let Some(proof) = vrf_proof_crypto {
            aether_types::VrfProof {
                output: proof.output,
                proof: proof.proof,
            }
        } else {
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            }
        };

        // Apply transactions speculatively (NOT committed to disk yet)
        let (receipts, overlay) = self.ledger.apply_block_speculatively_with_chain_id(
            &transactions,
            Some(self.chain_config.chain.chain_id_numeric),
        )?;
        let successful = receipts
            .iter()
            .filter(|r| matches!(r.status, aether_types::TransactionStatus::Success))
            .count();

        if !transactions.is_empty() {
            tracing::info!(
                successful,
                failed = receipts.len() - successful,
                "Speculative execution complete"
            );
        }

        // Compute block header roots from speculative state
        let state_root = overlay.state_root;
        let transactions_root = compute_transactions_root(&transactions);
        let receipts_root = compute_receipts_root(&receipts);

        let key = self
            .validator_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("validator_key required for block production"))?;
        let proposer_bytes = key.to_address();
        let proposer = aether_types::Address::from_slice(&proposer_bytes)
            .map_err(|e| anyhow::anyhow!("invalid proposer address: {e}"))?;

        let mut block = Block::new(
            slot,
            self.latest_block_hash,
            proposer,
            vrf_proof,
            transactions.clone(),
        );

        block.header.state_root = state_root;
        block.header.transactions_root = transactions_root;
        block.header.receipts_root = receipts_root;

        let block_hash = block.hash();
        tracing::info!(?block_hash, %state_root, "Block produced");

        // Validate our own block BEFORE committing state
        if let Err(e) = self.consensus.validate_block(&block) {
            // Discard overlay — state unchanged
            tracing::warn!(err = %e, "Self-produced block validation failed");
            return Ok(());
        }

        // Build stored receipts (with block context) for both cache and disk
        let stored_receipts: Vec<TransactionReceipt> = receipts
            .iter()
            .map(|r| {
                let mut sr = r.clone();
                sr.block_hash = block_hash;
                sr.slot = slot;
                sr
            })
            .collect();

        // ATOMIC COMMIT: overlay state + block + receipts in a single WriteBatch.
        // Previously these were two separate write_batch calls; a crash between
        // them could leave ledger state committed without the block record (or
        // vice versa), corrupting the node on restart.
        let mut batch = self.ledger.prepare_overlay_batch(&overlay)?;
        let block_batch = self.build_block_batch(&block, block_hash, &stored_receipts)?;
        batch.extend(block_batch);
        self.ledger.write_batch(batch)?;

        // Process fee market and credit proposer with priority fees
        let total_fees: u128 = transactions.iter().map(|tx| tx.fee).sum();
        let gas_used: u64 = transactions.iter().map(|tx| tx.gas_limit).sum();
        let fee_result = self.fee_market.process_block(gas_used, total_fees);

        // Credit proposer with their share (priority fees / tips)
        if fee_result.proposer_reward > 0 {
            if let Err(e) = self
                .ledger
                .credit_account(&block.header.proposer, fee_result.proposer_reward)
            {
                tracing::warn!(err = %e, "Failed to credit proposer fee reward");
            }
        }

        // Record burned fees in ledger (EIP-1559 deflationary mechanism)
        if fee_result.burned > 0 {
            if let Err(e) = self.ledger.record_burned_fees(fee_result.burned) {
                tracing::warn!(err = %e, "Failed to record burned fees");
            }
        }

        for sr in &stored_receipts {
            self.receipts.insert(sr.tx_hash, sr.clone());
        }

        self.fork_choice.add_block(slot, block_hash);
        self.latest_block_hash = block_hash;
        self.latest_block_slot = Some(slot);
        self.blocks_by_slot.insert(slot, block_hash);
        self.blocks_by_hash.insert(block_hash, block.clone());

        // Record block parent for 2-chain finality tracking
        self.consensus
            .record_block(block_hash, block.header.parent_hash, slot);

        // Remove transactions from mempool
        let tx_hashes: Vec<H256> = transactions.iter().map(|tx| tx.hash()).collect();
        self.mempool.remove_transactions(&tx_hashes);

        // Broadcast block to network
        self.broadcast(OutboundMessage::BroadcastBlock(block.clone()));

        // Vote on our own block
        self.vote_on_block(&block)?;

        Ok(())
    }

    /// Create a BLS vote for a block and submit to consensus + broadcast.
    fn vote_on_block(&mut self, block: &Block) -> Result<()> {
        let _span = tracing::info_span!("vote_on_block", slot = block.header.slot).entered();

        let (validator_key, bls_key) = match (&self.validator_key, &self.bls_key) {
            (Some(vk), Some(bk)) => (vk, bk),
            (Some(_), None) => {
                tracing::warn!("validator_key set but bls_key missing, cannot vote");
                return Ok(());
            }
            _ => return Ok(()), // Not a validator
        };

        let block_hash = block.hash();
        let slot = block.header.slot;
        let validator_pubkey = PublicKey::from_bytes(validator_key.public_key());

        let vote_msg = {
            let mut msg = Vec::new();
            msg.extend_from_slice(block_hash.as_bytes());
            msg.extend_from_slice(&slot.to_le_bytes());
            msg
        };
        let vote_sig = bls_key.sign(&vote_msg);

        // Use individual validator stake, NOT total_stake
        let voter_addr = validator_pubkey.to_address();
        let my_stake = self.consensus.validator_stake(&voter_addr);

        let vote = Vote {
            slot,
            block_hash,
            validator: validator_pubkey,
            signature: aether_types::Signature::from_bytes(vote_sig),
            stake: my_stake,
        };

        match self.consensus.add_vote(vote.clone()) {
            Ok(()) => tracing::info!(slot, ?block_hash, "Vote submitted"),
            Err(e) => tracing::warn!(slot, err = %e, "Vote failed"),
        }

        self.broadcast(OutboundMessage::BroadcastVote(vote));

        Ok(())
    }

    // ========================================================================
    // Block Reception (Phase B)
    // ========================================================================

    /// Handle a block received from the P2P network.
    pub fn on_block_received(&mut self, block: Block) -> Result<()> {
        let block_hash = block.hash();
        let _span = tracing::info_span!(
            "on_block_received",
            slot = block.header.slot,
            ?block_hash,
        )
        .entered();

        // Reject if already known (also prevents fee market double-update)
        if self.blocks_by_hash.contains_key(&block_hash) {
            return Ok(());
        }

        // Reject if slot is at or before finalized (except slot 0 edge case)
        if block.header.slot <= self.consensus.finalized_slot()
            && self.consensus.finalized_slot() > 0
        {
            return Ok(());
        }

        // Reject blocks with unsupported protocol version
        if block.header.version != aether_types::PROTOCOL_VERSION {
            bail!(
                "unsupported protocol version: got {}, expected {}",
                block.header.version,
                aether_types::PROTOCOL_VERSION
            );
        }

        // Buffer as orphan if parent is unknown (skip for genesis-like blocks).
        // We check this before full consensus validation because consensus checks
        // (e.g. future-slot rejection) may fail for blocks received out of order
        // during sync. These blocks will be fully validated when their parent arrives.
        if block.header.slot > 0
            && block.header.parent_hash != H256::zero()
            && !self.blocks_by_hash.contains_key(&block.header.parent_hash)
        {
            if self.orphan_count < MAX_ORPHAN_BLOCKS {
                tracing::info!(
                    slot = block.header.slot,
                    parent = ?block.header.parent_hash,
                    "Buffering orphan block (parent unknown)"
                );
                self.orphan_count += 1;
                self.orphan_blocks
                    .entry(block.header.parent_hash)
                    .or_default()
                    .push(block);
            } else {
                tracing::warn!(
                    slot = block.header.slot,
                    "Orphan buffer full ({MAX_ORPHAN_BLOCKS}), dropping block"
                );
            }
            return Ok(());
        }

        // Validate block via consensus (VRF proof, locked block check)
        self.consensus.validate_block(&block)?;

        // Validate slot monotonicity: block slot must be strictly greater than parent's slot
        if block.header.slot > 0 && block.header.parent_hash != H256::zero() {
            if let Some(parent_block) = self.blocks_by_hash.get(&block.header.parent_hash) {
                if block.header.slot <= parent_block.header.slot {
                    bail!(
                        "slot monotonicity violation: block slot {} <= parent slot {}",
                        block.header.slot,
                        parent_block.header.slot
                    );
                }
            }
        }

        // Validate transactions_root matches actual transactions (unconditional —
        // empty blocks naturally produce H256::zero() so zero roots still pass)
        let computed_tx_root = compute_transactions_root(&block.transactions);
        if computed_tx_root != block.header.transactions_root {
            bail!(
                "transactions_root mismatch: computed={}, block={}",
                computed_tx_root,
                block.header.transactions_root
            );
        }

        // Non-genesis blocks MUST carry a quorum certificate (aggregated vote).
        // Without this check, a malicious proposer could omit the QC entirely
        // and bypass BLS quorum verification.
        let is_genesis_or_bootstrap = block.header.slot <= 1
            || block.header.parent_hash == H256::zero();
        if !is_genesis_or_bootstrap && block.aggregated_vote.is_none() {
            bail!(
                "block at slot {} missing required quorum certificate (aggregated_vote)",
                block.header.slot
            );
        }

        // Verify BLS aggregate signature when present (proves quorum voted for parent)
        if let Some(ref agg_vote) = block.aggregated_vote {
            // The QC must reference this block's parent — it certifies that
            // a supermajority voted for the parent, justifying this extension.
            if !is_genesis_or_bootstrap && agg_vote.block_hash != block.header.parent_hash {
                bail!(
                    "aggregated vote references block {:?} but parent is {:?}",
                    agg_vote.block_hash,
                    block.header.parent_hash
                );
            }
            if agg_vote.signers.is_empty() {
                bail!("aggregated vote has no signers");
            }
            // Reconstruct the vote message: block_hash || slot (same as vote_on_block)
            let mut vote_msg = Vec::new();
            vote_msg.extend_from_slice(agg_vote.block_hash.as_bytes());
            vote_msg.extend_from_slice(&agg_vote.slot.to_le_bytes());

            // Look up BLS public keys and compute voted stake from our local
            // validator set. NEVER trust agg_vote.total_stake from the block —
            // an attacker could set it to any value to bypass quorum checks.
            let mut bls_pubkeys = Vec::with_capacity(agg_vote.signers.len());
            let mut voted_stake: u128 = 0;
            for signer in &agg_vote.signers {
                let addr = signer.to_address();
                let bls_pk = self.consensus.get_bls_pubkey(&addr).ok_or_else(|| {
                    anyhow::anyhow!("no BLS pubkey registered for signer {:?}", addr)
                })?;
                bls_pubkeys.push(bls_pk);
                voted_stake = voted_stake.saturating_add(self.consensus.validator_stake(&addr));
            }
            let agg_pk = aether_crypto_bls::aggregate_public_keys(&bls_pubkeys)
                .map_err(|e| anyhow::anyhow!("failed to aggregate signer pubkeys: {e}"))?;

            let valid = aether_crypto_bls::verify_aggregated(
                &agg_pk,
                &vote_msg,
                &agg_vote.aggregated_signature,
            )
            .map_err(|e| anyhow::anyhow!("BLS aggregate verification error: {e}"))?;
            if !valid {
                bail!("invalid BLS aggregate signature in block");
            }

            // Verify quorum: locally-computed voted stake must be >= 2/3 of total
            let total_stake = self.consensus.total_stake();
            if total_stake > 0 {
                let required = total_stake * 2 / 3 + 1;
                if voted_stake < required {
                    bail!(
                        "insufficient quorum: voted stake {} < required {} (2/3 of {})",
                        voted_stake,
                        required,
                        total_stake
                    );
                }
            }
        }

        // Execute transactions SPECULATIVELY (not committed to disk yet)
        // Use chain_id validation to reject cross-chain replay attacks
        let (receipts, overlay) = self.ledger.apply_block_speculatively_with_chain_id(
            &block.transactions,
            Some(self.chain_config.chain.chain_id_numeric),
        )?;

        // Validate receipts_root matches recomputed receipts
        let computed_receipts_root = compute_receipts_root(&receipts);
        if computed_receipts_root != block.header.receipts_root {
            bail!(
                "receipts_root mismatch: computed={}, block={}",
                computed_receipts_root,
                block.header.receipts_root
            );
        }

        // Validate state root matches before committing (unconditional)
        if overlay.state_root != block.header.state_root {
            // Discard overlay — state is UNCHANGED (rollback!)
            bail!(
                "state root mismatch: computed={}, block={} — block rejected, state unchanged",
                overlay.state_root,
                block.header.state_root
            );
        }

        // Build stored receipts (with block context) for both cache and disk
        let stored_receipts: Vec<TransactionReceipt> = receipts
            .iter()
            .map(|r| {
                let mut sr = r.clone();
                sr.block_hash = block_hash;
                sr.slot = block.header.slot;
                sr
            })
            .collect();

        // ATOMIC COMMIT: overlay state + block + receipts in a single WriteBatch.
        // Previously these were two separate write_batch calls; a crash between
        // them could leave ledger state committed without the block record,
        // corrupting the chain on restart.
        let mut batch = self.ledger.prepare_overlay_batch(&overlay)?;
        let block_batch = self.build_block_batch(&block, block_hash, &stored_receipts)?;
        batch.extend(block_batch);
        self.ledger.write_batch(batch)?;

        // Apply slash evidence: verify cryptographic proof before reducing stake.
        // Each SlashEvidence MUST carry two BLS-signed conflicting votes.
        // Evidence without valid proof is silently skipped (no block rejection)
        // so a single malformed entry cannot stall the chain.
        for evidence in &block.slash_evidence {
            // Require both votes and evidence type
            let (v1, v2, etype) = match (&evidence.vote1, &evidence.vote2, &evidence.evidence_type)
            {
                (Some(v1), Some(v2), Some(etype)) => (v1, v2, etype),
                _ => {
                    tracing::warn!(
                        validator = ?evidence.validator,
                        reason = %evidence.reason,
                        "Slash skipped — missing proof votes/type"
                    );
                    continue;
                }
            };

            // Build a SlashProof from the wire-format evidence
            let proof_type = match etype {
                aether_types::SlashEvidenceType::DoubleSign => SlashType::DoubleSign,
                aether_types::SlashEvidenceType::SurroundVote => SlashType::SurroundVote,
            };
            let proof = SlashProof {
                vote1: SlashVote {
                    slot: v1.slot,
                    block_hash: v1.block_hash,
                    validator: v1.validator,
                    validator_pubkey: v1.validator_pubkey.clone(),
                    signature: v1.signature.clone(),
                },
                vote2: SlashVote {
                    slot: v2.slot,
                    block_hash: v2.block_hash,
                    validator: v2.validator,
                    validator_pubkey: v2.validator_pubkey.clone(),
                    signature: v2.signature.clone(),
                },
                validator: evidence.validator,
                proof_type: proof_type.clone(),
            };

            // Cryptographically verify the proof (BLS signatures + structural checks)
            if let Err(e) = slash_verify::verify_slash_proof(&proof) {
                tracing::warn!(
                    validator = ?evidence.validator,
                    reason = %evidence.reason,
                    err = %e,
                    "Slash rejected — proof verification failed"
                );
                continue;
            }

            // Compute slash rate from the verified proof type — never trust
            // the untrusted slash_rate_bps field from the block.
            let validator_stake = self
                .staking_state
                .get_validator(&evidence.validator)
                .map(|v| v.staked_amount)
                .unwrap_or(0);
            let slash_amount =
                slash_verify::calculate_slash_amount(validator_stake, &proof.proof_type);
            // Convert to basis points: slash_amount / validator_stake * 10_000
            let rate_bps = if validator_stake > 0 {
                (slash_amount.saturating_mul(10_000) / validator_stake) as u32
            } else {
                0
            };

            match self.staking_state.slash(evidence.validator, u128::from(rate_bps)) {
                Ok(slashed) => tracing::warn!(
                    validator = ?evidence.validator,
                    rate_bps,
                    slashed,
                    reason = %evidence.reason,
                    "Slash applied"
                ),
                Err(e) => tracing::warn!(
                    validator = ?evidence.validator,
                    reason = %evidence.reason,
                    err = %e,
                    "Slash skipped"
                ),
            }
        }

        // Fork choice: track this block and check for competing forks
        let old_canonical = self.fork_choice.canonical_block(block.header.slot);
        let is_fork = self.fork_choice.add_block(block.header.slot, block_hash);
        let new_canonical = self.fork_choice.canonical_block(block.header.slot);

        if is_fork {
            tracing::warn!(
                slot = block.header.slot,
                "Fork detected — multiple blocks at same slot"
            );

            // If canonical choice changed, reorg mempool nonces for affected senders
            if old_canonical != new_canonical {
                if let Some(old_hash) = old_canonical {
                    if let Some(old_block) = self.blocks_by_hash.get(&old_hash) {
                        let reverted_txs = old_block.transactions.clone();
                        let mut new_nonces = std::collections::HashMap::new();
                        for tx in &block.transactions {
                            new_nonces
                                .entry(tx.sender)
                                .and_modify(|n: &mut u64| *n = (*n).max(tx.nonce + 1))
                                .or_insert(tx.nonce + 1);
                        }
                        self.mempool.reorg(reverted_txs, new_nonces);
                    }
                }
            }
        }

        // Store block in memory
        self.blocks_by_slot.insert(block.header.slot, block_hash);
        self.blocks_by_hash.insert(block_hash, block.clone());

        // Only update tip if this is the canonical choice
        if new_canonical == Some(block_hash) {
            self.latest_block_hash = block_hash;
            self.latest_block_slot = Some(block.header.slot);
        }

        // Record block parent for 2-chain finality tracking
        self.consensus
            .record_block(block_hash, block.header.parent_hash, block.header.slot);

        for sr in &stored_receipts {
            self.receipts.insert(sr.tx_hash, sr.clone());
        }

        // Remove included txs from mempool
        let tx_hashes: Vec<H256> = block.transactions.iter().map(|tx| tx.hash()).collect();
        self.mempool.remove_transactions(&tx_hashes);

        // Update fee market, credit proposer, record burns
        let total_fees: u128 = block.transactions.iter().map(|tx| tx.fee).sum();
        let gas_used: u64 = block.transactions.iter().map(|tx| tx.gas_limit).sum();
        let fee_result = self.fee_market.process_block(gas_used, total_fees);

        if fee_result.proposer_reward > 0 {
            let _ = self
                .ledger
                .credit_account(&block.header.proposer, fee_result.proposer_reward);
        }
        if fee_result.burned > 0 {
            let _ = self.ledger.record_burned_fees(fee_result.burned);
        }

        // Vote on this block (if we're a validator)
        self.vote_on_block(&block)?;

        // Try to apply any orphan blocks that were waiting for this block as parent.
        self.process_orphans(block_hash);

        // Update sync state based on network tip vs local tip
        if let Some(local_slot) = self.latest_block_slot {
            self.sync_manager.check_sync_needed(local_slot, block.header.slot);
        }

        Ok(())
    }

    /// Recursively process orphan blocks whose parent has just been applied.
    fn process_orphans(&mut self, parent_hash: H256) {
        if let Some(orphans) = self.orphan_blocks.remove(&parent_hash) {
            let count = orphans.len();
            self.orphan_count = self.orphan_count.saturating_sub(count);
            for orphan in orphans {
                let slot = orphan.header.slot;
                match self.on_block_received(orphan) {
                    Ok(()) => tracing::info!(slot, "Applied previously orphaned block"),
                    Err(e) => tracing::warn!(slot, err = %e, "Orphaned block rejected"),
                }
            }
        }
    }

    /// Returns whether the node is currently syncing.
    pub fn is_syncing(&self) -> bool {
        self.sync_manager.is_syncing()
    }

    /// Returns the number of buffered orphan blocks.
    pub fn orphan_count(&self) -> usize {
        self.orphan_count
    }

    // ========================================================================
    // Vote Reception (Phase C)
    // ========================================================================

    /// Handle a vote received from the P2P network.
    /// Checks for double-signing before processing. If a validator votes for two
    /// different blocks in the same slot, they are slashed (5% of stake).
    pub fn on_vote_received(&mut self, vote: Vote) -> Result<()> {
        let validator_address = vote.validator.to_address();

        // Check for double-signing before accepting the vote
        if let Some(proof) = self.slashing_detector.record_vote(
            validator_address,
            vote.validator.clone(),
            vote.slot,
            vote.block_hash,
            vote.signature.clone(),
        ) {
            // Double-sign detected! Slash 5% (500 basis points)
            let slashed = self.consensus.slash_validator(&proof.validator, 500);
            tracing::warn!(
                validator = ?proof.validator,
                slot = vote.slot,
                slashed,
                "Double-sign detected — slashed 5% of stake"
            );
        }

        self.consensus.add_vote(vote)?;
        self.check_finality();
        Ok(())
    }

    // ========================================================================
    // Network Event Dispatch
    // ========================================================================

    /// Handle a raw network event from the P2P layer.
    pub fn handle_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match decode_network_event(event) {
            Some(NodeMessage::BlockReceived(block)) => {
                if let Err(e) = self.on_block_received(block) {
                    tracing::debug!(err = %e, "Block rejected");
                }
            }
            Some(NodeMessage::VoteReceived(vote)) => {
                if let Err(e) = self.on_vote_received(vote) {
                    tracing::debug!(err = %e, "Vote rejected");
                }
            }
            Some(NodeMessage::TransactionReceived(tx)) => {
                if let Err(e) = self.mempool.add_transaction(tx) {
                    tracing::debug!(err = %e, "Tx rejected");
                }
            }
            None => {}
        }
        Ok(())
    }

    fn check_finality(&mut self) {
        let current_slot = self.consensus.current_slot();
        let last_finalized = self.consensus.finalized_slot();

        // Only check slots we haven't checked yet (avoid O(n) scan on restart)
        let start = if last_finalized > 0 {
            last_finalized
        } else {
            0
        };

        // Limit to checking at most 100 slots per tick to prevent CPU spikes
        let end = current_slot.min(start + 100);

        for slot in start..=end {
            if self.consensus.check_finality(slot) {
                tracing::info!(slot, "Slot finalized via VRF+HotStuff+BLS");

                // Update epoch randomness ONCE per epoch from the first finalized block
                // of that epoch to ensure deterministic randomness across all nodes.
                let finalized_epoch = slot / self.chain_config.chain.epoch_slots;
                let epoch_start = finalized_epoch * self.chain_config.chain.epoch_slots;
                if slot == epoch_start {
                    if let Some(block) = self.get_block_by_slot(slot) {
                        if block.header.vrf_proof.output != [0u8; 32] {
                            self.consensus
                                .update_epoch_randomness(&block.header.vrf_proof.output);
                        }
                    }
                }

                // Finalize in fork choice
                if let Some(&hash) = self.blocks_by_slot.get(&slot) {
                    if !self.fork_choice.finalize(slot, hash) {
                        tracing::warn!(
                            slot,
                            ?hash,
                            "fork_choice: could not finalize unknown block"
                        );
                    }
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn get_state_root(&self) -> H256 {
        self.ledger.state_root()
    }

    pub fn mempool_size(&self) -> usize {
        self.mempool.len()
    }

    pub fn poh_metrics(&self) -> Option<&PohMetrics> {
        self.last_poh_metrics.as_ref()
    }

    pub fn current_slot(&self) -> Slot {
        self.consensus.current_slot()
    }

    pub fn finalized_slot(&self) -> Slot {
        self.consensus.finalized_slot()
    }

    pub fn latest_block_slot(&self) -> Option<Slot> {
        self.latest_block_slot
    }

    pub fn allows_airdrop(&self) -> bool {
        matches!(
            self.chain_config.chain.chain_id.as_str(),
            "aether-dev-1" | "aether-testnet-1"
        )
    }

    pub fn seed_account(&mut self, address: &Address, balance: u128) -> Result<()> {
        self.ledger.seed_account(address, balance)
    }

    pub fn get_block_by_slot(&self, slot: Slot) -> Option<Block> {
        self.blocks_by_slot
            .get(&slot)
            .and_then(|hash| self.blocks_by_hash.get(hash))
            .cloned()
    }

    pub fn get_block_by_hash(&self, hash: H256) -> Option<Block> {
        self.blocks_by_hash.get(&hash).cloned()
    }

    pub fn get_transaction_receipt(&self, tx_hash: H256) -> Option<TransactionReceipt> {
        self.receipts.get(&tx_hash).cloned()
    }

    pub fn get_account(&self, address: Address) -> Result<Option<Account>> {
        self.ledger.get_account(&address)
    }

    pub fn base_fee(&self) -> u128 {
        self.fee_market.base_fee
    }

    /// Mutable access to the in-memory staking state.
    ///
    /// Used by tests and the genesis bootstrap path to register validators
    /// before block production begins.
    pub fn staking_state_mut(&mut self) -> &mut StakingState {
        &mut self.staking_state
    }

    /// Read-only access to the in-memory staking state.
    pub fn staking_state(&self) -> &StakingState {
        &self.staking_state
    }
}

// ============================================================================
// Block Header Root Computation (Phase D)
// ============================================================================

/// Compute the Merkle root of a list of transactions (hash of hashes).
pub fn compute_transactions_root(txs: &[Transaction]) -> H256 {
    if txs.is_empty() {
        return H256::zero();
    }
    let mut hasher = Sha256::new();
    for tx in txs {
        hasher.update(tx.hash().as_bytes());
    }
    H256::from_slice(&hasher.finalize()).unwrap()
}

/// Compute the Merkle root of a list of receipts.
///
/// Uses only the deterministic fields (tx_hash, status, gas_used, logs, state_root)
/// and excludes block_hash/slot which are set AFTER root computation. This ensures
/// the root is consistent regardless of when it's computed.
pub fn compute_receipts_root(receipts: &[TransactionReceipt]) -> H256 {
    if receipts.is_empty() {
        return H256::zero();
    }
    let mut hasher = Sha256::new();
    for receipt in receipts {
        // Hash only deterministic fields to avoid non-determinism from
        // block_hash/slot being set after root computation.
        let mut receipt_hasher = Sha256::new();
        receipt_hasher.update(receipt.tx_hash.as_bytes());
        receipt_hasher.update(bincode::serialize(&receipt.status).unwrap_or_default());
        receipt_hasher.update(receipt.gas_used.to_le_bytes());
        receipt_hasher.update(bincode::serialize(&receipt.logs).unwrap_or_default());
        receipt_hasher.update(receipt.state_root.as_bytes());
        hasher.update(receipt_hasher.finalize());
    }
    H256::from_slice(&hasher.finalize()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_consensus::SimpleConsensus;
    use aether_types::{PublicKey, ValidatorInfo};
    use tempfile::TempDir;

    fn validator_info_from_key(keypair: &Keypair) -> ValidatorInfo {
        ValidatorInfo {
            pubkey: PublicKey::from_bytes(keypair.public_key()),
            stake: 1_000,
            commission: 0,
            active: true,
        }
    }

    #[test]
    fn updates_poh_metrics_each_slot() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));

        let mut node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        node.process_slot().unwrap();
        let first_metrics = node.poh_metrics().cloned().unwrap();
        assert_eq!(first_metrics.tick_count, 1);

        node.process_slot().unwrap();
        let second_metrics = node.poh_metrics().cloned().unwrap();
        assert!(second_metrics.tick_count >= 2);
        assert!(second_metrics.average_duration_ms >= 0.0);
    }

    #[test]
    fn outbound_buffer_is_capped() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));
        let mut node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        // Push more than MAX_OUTBOUND_BUFFER messages
        for _ in 0..MAX_OUTBOUND_BUFFER + 100 {
            node.broadcast(OutboundMessage::BroadcastVote(Vote {
                slot: 0,
                block_hash: H256::zero(),
                validator: PublicKey::from_bytes(vec![0u8; 32]),
                signature: aether_types::Signature::from_bytes(vec![0u8; 64]),
                stake: 0,
            }));
        }

        assert_eq!(node.outbound_buffer.len(), MAX_OUTBOUND_BUFFER);
    }

    #[test]
    fn duplicate_block_is_ignored() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));
        let mut node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        let block = Block::new(
            0,
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );

        // Insert manually
        let hash = block.hash();
        node.blocks_by_hash.insert(hash, block.clone());

        // on_block_received should return Ok (silently skip duplicate)
        assert!(node.on_block_received(block).is_ok());
    }

    #[test]
    fn transactions_root_is_deterministic() {
        let tx1 = Transaction {
            nonce: 0,
            chain_id: 1,
            sender: Address::from_slice(&[1u8; 20]).unwrap(),
            sender_pubkey: PublicKey::from_bytes(vec![1u8; 32]),
            inputs: vec![],
            outputs: vec![],
            reads: std::collections::HashSet::new(),
            writes: std::collections::HashSet::new(),
            program_id: None,
            data: vec![1, 2, 3],
            gas_limit: 21000,
            fee: 1000,
            signature: aether_types::Signature::from_bytes(vec![0u8; 64]),
        };

        let root1 = compute_transactions_root(std::slice::from_ref(&tx1));
        let root2 = compute_transactions_root(&[tx1]);
        assert_eq!(root1, root2, "same input must produce same root");
    }

    #[test]
    fn empty_root_is_zero() {
        assert_eq!(compute_transactions_root(&[]), H256::zero());
        assert_eq!(compute_receipts_root(&[]), H256::zero());
    }

    #[test]
    fn orphan_block_is_buffered_when_parent_unknown() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));
        let mut node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        let unknown_parent = H256::from_slice(&[0xAB; 32]).unwrap();
        let orphan = Block::new(
            5,
            unknown_parent,
            Address::from_slice(&[1u8; 20]).unwrap(),
            aether_types::VrfProof {
                output: [0u8; 32],
                proof: vec![],
            },
            vec![],
        );

        // Should succeed (buffered, not rejected)
        assert!(node.on_block_received(orphan).is_ok());
        assert_eq!(node.orphan_count(), 1);
        assert!(node.orphan_blocks.contains_key(&unknown_parent));
    }

    #[test]
    fn orphan_buffer_respects_max_capacity() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));
        let mut node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        // Fill up the orphan buffer
        for i in 0..MAX_ORPHAN_BLOCKS + 10 {
            let parent = H256::from_slice(&[(i & 0xFF) as u8; 32]).unwrap();
            let orphan = Block::new(
                (i + 5) as u64,
                parent,
                Address::from_slice(&[1u8; 20]).unwrap(),
                aether_types::VrfProof {
                    output: [0u8; 32],
                    proof: vec![],
                },
                vec![],
            );
            let _ = node.on_block_received(orphan);
        }

        // Should be capped
        assert!(node.orphan_count() <= MAX_ORPHAN_BLOCKS);
    }

    #[test]
    fn sync_manager_tracks_state() {
        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));
        let node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        // Node starts as synced
        assert!(!node.is_syncing());
    }

    #[test]
    fn epoch_transition_completes_unbonding_and_credits_account() {
        use aether_program_staking::Unbonding;
        use aether_types::Address;

        let temp_dir = TempDir::new().unwrap();
        let keypair = Keypair::generate();
        let validators = vec![validator_info_from_key(&keypair)];
        let consensus = Box::new(SimpleConsensus::new(validators));
        let mut node = Node::new(
            temp_dir.path(),
            consensus,
            Some(keypair),
            None,
            Arc::new(ChainConfig::devnet()),
        )
        .unwrap();

        // Set up an unbonding entry that completes at slot 0 (already elapsed).
        let delegator = Address::from(aether_types::H160([0x42u8; 20]));
        let unbond_amount = 5_000u128;
        node.staking_state_mut().unbonding.push(Unbonding {
            address: delegator,
            validator: Address::from(aether_types::H160([0x01u8; 20])),
            amount: unbond_amount,
            complete_slot: 0,
        });

        // Seed the ledger account so credit_account has something to update.
        node.ledger.credit_account(&delegator, 0).ok();

        // Trigger epoch transition; slot=0 means complete_slot<=current_slot.
        node.process_epoch_transition(1).unwrap();

        // Unbonding queue should be empty — entry was consumed.
        assert!(node.staking_state().unbonding.is_empty());

        // Delegator's account should have been credited.
        let account = node.ledger.get_account(&delegator).unwrap().unwrap();
        assert_eq!(account.balance, unbond_amount);
    }
}
