// ============================================================================
// AETHER TEE VERIFIER - Trusted Execution Environment Attestation
// ============================================================================
// PURPOSE: Verify that AI workers run in genuine TEEs
//
// SUPPORTED TEES:
// - AMD SEV-SNP: Secure Encrypted Virtualization
// - Intel TDX: Trust Domain Extensions
// - AWS Nitro: Nitro Enclaves
//
// ATTESTATION FLOW:
// 1. Worker boots in TEE
// 2. TEE measures code + data → measurement
// 3. Worker requests attestation report from TEE
// 4. Report includes: measurement, timestamp, signature
// 5. Worker sends report to validators
// 6. Validators verify:
//    - Signature chain (root CA → TEE cert → report)
//    - Measurement matches approved build
//    - Timestamp is fresh (<60s)
//    - Nonce prevents replay
//
// SECURITY PROPERTIES:
// - Code integrity: Measurement proves exact code running
// - Confidentiality: Data encrypted in TEE memory
// - Freshness: Recent attestation prevents replay
// - Non-repudiation: TEE signs attestation
//
// INTEGRATION:
// - Job escrow checks attestation before assigning work
// - Staking slashes workers with invalid attestations
// - Reputation tracks attestation failures
// ============================================================================

pub mod attestation;

pub use attestation::{TeeType, AttestationReport, TeeVerifier};
