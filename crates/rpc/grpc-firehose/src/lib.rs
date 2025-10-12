// ============================================================================
// AETHER GRPC FIREHOSE - High-Throughput Block Streaming
// ============================================================================
// PURPOSE: Stream blocks/txs/events to indexers at full chain speed
//
// INSPIRED BY: Solana Geyser, The Graph Firehose
//
// FEATURES:
// - Streaming blocks (forward & backward)
// - Filter by account/program
// - Checkpoint resume
// - Parallel streams
//
// USAGE:
//   Indexer connects → subscribes to block stream → processes events
// ============================================================================

pub mod firehose;
pub mod streaming;

pub use firehose::FirehoseServer;

