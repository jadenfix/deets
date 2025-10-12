// ============================================================================
// AETHER INDEXER - Blockchain Data Indexer
// ============================================================================
// PURPOSE: Ingest blocks/txs/events from Firehose → Postgres → GraphQL/REST
//
// ARCHITECTURE:
//   Firehose gRPC → Indexer → Postgres → GraphQL API → Frontend/Users
//
// TABLES:
//   blocks, transactions, accounts, utxos, jobs, vcrs, votes
//
// FEATURES:
//   - Real-time ingestion
//   - Historical backfill
//   - GraphQL queries
//   - REST API
// ============================================================================

fn main() {
    println!("Aether Indexer v0.1.0");
}
