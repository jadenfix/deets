// ============================================================================
// AETHER RUST SDK - Client Library
// ============================================================================
// PURPOSE: Ergonomic Rust API for building Aether applications
//
// FEATURES:
//   - Transaction building
//   - Account management
//   - RPC client
//   - Contract calls
//   - AI job submission
//
// EXAMPLE:
// ```
// let client = AetherClient::new("http://localhost:8545");
// let keypair = Keypair::generate();
//
// // Transfer AIC
// let tx = client.transfer()
//     .to(recipient)
//     .amount(1000)
//     .token(TokenType::AIC)
//     .build()?;
//
// let result = client.submit(tx).await?;
// ```
// ============================================================================

pub mod client;
pub mod job_builder;
pub mod transaction_builder;
pub mod types;

// TODO: Add a custom `AetherSdkError` enum instead of relying on `anyhow::Error`
// throughout the public API. This would give callers typed error matching.

pub use client::AetherClient;
pub use job_builder::JobBuilder;
