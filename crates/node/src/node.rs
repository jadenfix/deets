use aether_consensus::ConsensusEngine;
use aether_crypto_bls::BlsKeypair;
use aether_crypto_primitives::Keypair;
use aether_ledger::{EmissionSchedule, FeeMarket, Ledger};
use aether_mempool::Mempool;
use aether_p2p::network::NetworkEvent;
use aether_state_storage::{Storage, CF_BLOCKS, CF_METADATA, CF_RECEIPTS};
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

const MAX_OUTBOUND_BUFFER: usize = 10_000;
const MAX_CACHED_BLOCKS: usize = 10_000;
const MAX_CACHED_RECEIPTS: usize = 50_000;

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
    /// Channel to send outbound messages (blocks, votes, txs) to P2P layer.
    broadcast_tx: Option<mpsc::Sender<OutboundMessage>>,
    /// Collected outbound messages when no broadcast channel is set (for testing).
    outbound_buffer: Vec<OutboundMessage>,
    /// Consecutive timeout counter for circuit breaker.
    consecutive_timeouts: u32,
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
        let mempool = Mempool::new(chain_config.fees.clone());

        // Warn on asymmetric key configuration
        if validator_key.is_some() != bls_key.is_some() {
            println!(
                "WARNING: asymmetric key config — validator_key={}, bls_key={}. Voting will be disabled.",
                validator_key.is_some(),
                bls_key.is_some()
            );
        }

        // Load persisted blocks from disk
        let (blocks_by_slot, blocks_by_hash, latest_block_hash, latest_block_slot) =
            Self::load_blocks_from_storage(ledger.storage())?;

        if !blocks_by_hash.is_empty() {
            println!(
                "Recovered {} blocks from disk (tip: slot {:?})",
                blocks_by_hash.len(),
                latest_block_slot
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
            broadcast_tx: None,
            outbound_buffer: Vec::new(),
            consecutive_timeouts: 0,
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

    /// Persist a block and its receipts to disk.
    /// Persist a block and its receipts to disk in a SINGLE atomic batch.
    fn persist_block(
        &self,
        block: &Block,
        block_hash: H256,
        receipts: &[TransactionReceipt],
    ) -> Result<()> {
        use aether_state_storage::StorageBatch;
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

        // Atomic commit — all-or-nothing
        self.ledger.storage().write_batch(batch)?;
        Ok(())
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
                Err(mpsc::error::TrySendError::Full(msg)) => {
                    // Backpressure: P2P layer can't keep up, drop the message
                    eprintln!(
                        "WARNING: P2P outbound channel full, dropping {:?}",
                        std::mem::discriminant(&msg)
                    );
                }
                Err(mpsc::error::TrySendError::Closed(msg)) => {
                    // Channel closed — fall back to buffer so message isn't lost
                    eprintln!("WARNING: broadcast channel closed");
                    if self.outbound_buffer.len() < MAX_OUTBOUND_BUFFER {
                        self.outbound_buffer.push(msg);
                    }
                }
            }
        } else if self.outbound_buffer.len() < MAX_OUTBOUND_BUFFER {
            self.outbound_buffer.push(msg);
        } else {
            eprintln!("CRITICAL: outbound buffer full ({MAX_OUTBOUND_BUFFER}), dropping message");
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

        println!("Node starting...");
        println!("Validator: {}", self.validator_key.is_some());
        println!("Starting slot: {}", self.consensus.current_slot());

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
                eprintln!(
                    "CIRCUIT BREAKER: {} consecutive timeouts at slot {} — possible network partition or all peers down",
                    self.consecutive_timeouts, slot
                );
            }
            println!(
                "Slot {}: TIMEOUT ({} consecutive) — advancing via pacemaker",
                slot, self.consecutive_timeouts
            );
            self.consensus.on_timeout();
        } else {
            self.consecutive_timeouts = 0;
        }

        let metrics = self.poh.tick(Instant::now());
        self.last_poh_metrics = Some(metrics.clone());
        println!(
            "PoH tick {} ms avg {:.1} jitter {:.1}",
            metrics.last_duration_ms, metrics.average_duration_ms, metrics.jitter_ms
        );

        if let Some(ref keypair) = self.validator_key {
            let pubkey = PublicKey::from_bytes(keypair.public_key());

            if self.consensus.is_leader(slot, &pubkey) {
                println!("Slot {}: I am leader, producing block", slot);
                self.produce_block(slot)?;
            } else {
                println!("Slot {}: Not leader, waiting for block", slot);
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

        println!(
            "Epoch {} → {}: distributing {} emission rewards",
            new_epoch - 1,
            new_epoch,
            emission
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
                let my_share = (emission * my_stake) / total_stake;
                if my_share > 0 {
                    if let Err(e) = self.ledger.credit_account(&my_addr, my_share) {
                        eprintln!("WARNING: failed to credit emission reward: {e}");
                    }
                }
            }
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

        // Prune fork choice for finalized slots (no longer need candidates)
        let finalized = self.consensus.finalized_slot();
        self.fork_choice.prune_before(finalized);
    }

    fn produce_block(&mut self, slot: Slot) -> Result<()> {
        // Forced inclusion: include txs that have been waiting too long (anti-censorship)
        let forced = self
            .mempool
            .must_include_transactions(slot, self.fee_market.base_fee);
        let forced_count = forced.len();
        let remaining_capacity = 1000usize.saturating_sub(forced_count);
        let regular = self.mempool.get_transactions(remaining_capacity, 5_000_000);
        let transactions = if forced_count > 0 {
            println!("  Forced inclusion: {} txs", forced_count);
            let mut all = forced;
            all.extend(regular);
            all
        } else {
            regular
        };

        if transactions.is_empty() {
            println!("  No transactions to include");
        }

        println!("  Including {} transactions", transactions.len());

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
            println!(
                "  {} successful, {} failed",
                successful,
                receipts.len() - successful
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
        println!("  Block produced: {:?}", block_hash);
        println!("  State root: {}", state_root);

        // Validate our own block BEFORE committing state
        if let Err(e) = self.consensus.validate_block(&block) {
            // Discard overlay — state unchanged
            println!("  WARNING: Block validation failed: {}", e);
            return Ok(());
        }

        // Validation passed — NOW commit state to disk
        self.ledger.commit_overlay(overlay)?;

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
                eprintln!("WARNING: failed to credit proposer fee reward: {e}");
            }
        }

        // Record burned fees in ledger (EIP-1559 deflationary mechanism)
        if fee_result.burned > 0 {
            if let Err(e) = self.ledger.record_burned_fees(fee_result.burned) {
                eprintln!("WARNING: failed to record burned fees: {e}");
            }
        }

        // Build stored receipts once (with block context), use for both cache and disk
        let stored_receipts: Vec<TransactionReceipt> = receipts
            .iter()
            .map(|r| {
                let mut sr = r.clone();
                sr.block_hash = block_hash;
                sr.slot = slot;
                sr
            })
            .collect();

        for sr in &stored_receipts {
            self.receipts.insert(sr.tx_hash, sr.clone());
        }

        self.fork_choice.add_block(slot, block_hash);
        self.latest_block_hash = block_hash;
        self.latest_block_slot = Some(slot);
        self.blocks_by_slot.insert(slot, block_hash);
        self.blocks_by_hash.insert(block_hash, block.clone());

        // Persist block to disk
        self.persist_block(&block, block_hash, &stored_receipts)?;

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
        let (validator_key, bls_key) = match (&self.validator_key, &self.bls_key) {
            (Some(vk), Some(bk)) => (vk, bk),
            (Some(_), None) => {
                println!("  WARNING: validator_key set but bls_key missing, cannot vote");
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
            Ok(()) => println!("  Vote created and processed"),
            Err(e) => println!("  Vote failed: {e}"),
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

        // Validate parent exists (skip for genesis-like blocks)
        if block.header.slot > 0
            && block.header.parent_hash != H256::zero()
            && !self.blocks_by_hash.contains_key(&block.header.parent_hash)
        {
            bail!(
                "unknown parent block: {:?} for slot {}",
                block.header.parent_hash,
                block.header.slot
            );
        }

        // Verify BLS aggregate signature when present (proves quorum voted for parent)
        if let Some(ref agg_vote) = block.aggregated_vote {
            if agg_vote.signers.is_empty() {
                bail!("aggregated vote has no signers");
            }
            // Reconstruct the vote message: block_hash || slot (same as vote_on_block)
            let mut vote_msg = Vec::new();
            vote_msg.extend_from_slice(agg_vote.block_hash.as_bytes());
            vote_msg.extend_from_slice(&agg_vote.slot.to_le_bytes());

            // Look up BLS public keys for each signer via their Ed25519 identity
            let mut bls_pubkeys = Vec::with_capacity(agg_vote.signers.len());
            for signer in &agg_vote.signers {
                let addr = signer.to_address();
                let bls_pk = self.consensus.get_bls_pubkey(&addr).ok_or_else(|| {
                    anyhow::anyhow!("no BLS pubkey registered for signer {:?}", addr)
                })?;
                bls_pubkeys.push(bls_pk);
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

            // Verify quorum: aggregated stake must be >= 2/3 of total stake
            let total_stake = self.consensus.total_stake();
            if total_stake > 0 {
                let required = total_stake * 2 / 3 + 1;
                if agg_vote.total_stake < required {
                    bail!(
                        "insufficient quorum: aggregated stake {} < required {} (2/3 of {})",
                        agg_vote.total_stake,
                        required,
                        total_stake
                    );
                }
            }
        }

        // Execute transactions SPECULATIVELY (not committed to disk yet)
        let (receipts, overlay) = self.ledger.apply_block_speculatively(&block.transactions)?;

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

        // State root matches — commit overlay to permanent storage
        self.ledger.commit_overlay(overlay)?;

        // Fork choice: track this block and check for competing forks
        let old_canonical = self.fork_choice.canonical_block(block.header.slot);
        let is_fork = self.fork_choice.add_block(block.header.slot, block_hash);
        let new_canonical = self.fork_choice.canonical_block(block.header.slot);

        if is_fork {
            println!(
                "  FORK detected at slot {}: multiple blocks",
                block.header.slot
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

        // Store block
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

        // Build stored receipts once (with block context), use for both cache and disk
        let stored_receipts: Vec<TransactionReceipt> = receipts
            .iter()
            .map(|r| {
                let mut sr = r.clone();
                sr.block_hash = block_hash;
                sr.slot = block.header.slot;
                sr
            })
            .collect();
        for sr in &stored_receipts {
            self.receipts.insert(sr.tx_hash, sr.clone());
        }
        self.persist_block(&block, block_hash, &stored_receipts)?;

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

        Ok(())
    }

    // ========================================================================
    // Vote Reception (Phase C)
    // ========================================================================

    /// Handle a vote received from the P2P network.
    pub fn on_vote_received(&mut self, vote: Vote) -> Result<()> {
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
                    println!("  Block rejected: {e}");
                }
            }
            Some(NodeMessage::VoteReceived(vote)) => {
                if let Err(e) = self.on_vote_received(vote) {
                    println!("  Vote rejected: {e}");
                }
            }
            Some(NodeMessage::TransactionReceived(tx)) => {
                if let Err(e) = self.mempool.add_transaction(tx) {
                    println!("  Tx rejected: {e}");
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
                println!("✓ FINALIZED: Slot {} via VRF+HotStuff+BLS!", slot);

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
                        eprintln!(
                            "WARN: fork_choice: could not finalize unknown block {hash:?} at slot {slot}"
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
}
