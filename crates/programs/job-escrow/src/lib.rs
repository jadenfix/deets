// ============================================================================
// AETHER JOB ESCROW PROGRAM - AI Credits Marketplace
// ============================================================================
// PURPOSE: Trustless escrow for AI inference jobs paid in AIC
//
// TOKEN: AIC (AI Credits, burned on successful job completion)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    JOB ESCROW SYSTEM                              │
// ├──────────────────────────────────────────────────────────────────┤
// │  User Posts Job  →  AIC Escrowed  →  Router Selects Provider     │
// │         ↓                                      ↓                  │
// │  Provider Accepts  →  Stakes Bond  →  Executes in TEE            │
// │         ↓                                      ↓                  │
// │  VCR Submitted  →  Challenge Window  →  Watchtower Verification  │
// │         ↓                                      ↓                  │
// │  Settlement  →  Burn AIC  →  Pay Provider  →  Return Bond        │
// └──────────────────────────────────────────────────────────────────┘
//
// JOB STATE:
// ```
// struct Job:
//     id: H256
//     requester: Address
//     model_hash: H256
//     code_hash: H256
//     input_hash: H256
//     units: u64  // AI compute units
//     max_price_per_unit: u128  // In AIC
//     sla_ms: u64  // Latency requirement
//     deadline_slot: u64
//     status: JobStatus
//     provider: Option<Address>
//     provider_bond: u128
//     escrow_amount: u128
//     vcr: Option<Vcr>
//     challenge_window_end: Option<u64>
//
// enum JobStatus:
//     Posted
//     Accepted
//     VcrSubmitted
//     Challenged
//     Completed
//     Failed
//     Disputed
// ```
//
// WORKFLOW:
// ```
// fn post_job(model_hash, code_hash, input_data, units, max_price, sla, deadline):
//     input_hash = hash(input_data)
//     
//     // Calculate escrow amount
//     escrow_amount = units * max_price
//     
//     // Lock AIC in escrow
//     transfer_aic(caller, ESCROW_ACCOUNT, escrow_amount)
//     
//     job = Job {
//         id: hash(caller || input_hash || timestamp),
//         requester: caller,
//         model_hash: model_hash,
//         code_hash: code_hash,
//         input_hash: input_hash,
//         units: units,
//         max_price_per_unit: max_price,
//         sla_ms: sla,
//         deadline_slot: deadline,
//         status: Posted,
//         escrow_amount: escrow_amount
//     }
//     
//     store_job(job)
//     emit_event(JobPosted { job_id: job.id })
//     
//     return job.id
//
// fn accept_job(job_id):
//     job = get_job(job_id)
//     require(job.status == Posted)
//     
//     // Provider stakes bond (slashable if fraud)
//     bond = max(VCR_BOND_MINIMUM, job.escrow_amount * BOND_RATIO)
//     transfer_aic(caller, ESCROW_ACCOUNT, bond)
//     
//     job.provider = caller
//     job.provider_bond = bond
//     job.status = Accepted
//     
//     emit_event(JobAccepted { job_id: job.id, provider: caller })
//
// fn submit_vcr(job_id, vcr):
//     job = get_job(job_id)
//     require(job.status == Accepted)
//     require(caller == job.provider)
//     require(current_slot <= job.deadline_slot)
//     
//     // Validate VCR structure
//     require(vcr.job_id == job_id)
//     require(vcr.model_hash == job.model_hash)
//     require(vcr.code_hash == job.code_hash)
//     require(vcr.input_hash == job.input_hash)
//     
//     // Verify TEE attestation
//     require(verify_tee_quote(vcr.tee_quote))
//     
//     // Verify KZG commitments (if present)
//     if vcr.kzg_commits.is_some():
//         require(verify_kzg_structure(vcr.kzg_commits))
//     
//     job.vcr = vcr
//     job.status = VcrSubmitted
//     job.challenge_window_end = current_slot + VCR_CHALLENGE_WINDOW
//     
//     emit_event(VcrSubmitted { job_id: job.id })
//
// fn challenge_vcr(job_id, challenge_proof):
//     job = get_job(job_id)
//     require(job.status == VcrSubmitted)
//     require(current_slot <= job.challenge_window_end)
//     
//     // Verify challenge proof
//     match challenge_proof:
//         InvalidTeeQuote(proof):
//             if verify_invalid_tee_proof(proof):
//                 slash_provider(job)
//         
//         InvalidKzgOpening(indices, expected_values):
//             if verify_kzg_opening_invalid(job.vcr.kzg_commits, indices, expected_values):
//                 slash_provider(job)
//         
//         RedundancyMismatch(other_vcrs):
//             if verify_redundancy_quorum_mismatch(job.vcr, other_vcrs):
//                 slash_provider(job)
//     
//     job.status = Disputed
//
// fn settle(job_id):
//     job = get_job(job_id)
//     require(job.status == VcrSubmitted)
//     require(current_slot > job.challenge_window_end)
//     
//     // No challenges → job successful
//     
//     // Burn AIC (deflationary mechanism)
//     burn_aic(job.escrow_amount)
//     
//     // Pay provider in stablecoin or AIC (from separate provider fund)
//     payment = calculate_provider_payment(job)
//     transfer_payment(PROVIDER_FUND, job.provider, payment)
//     
//     // Return provider bond
//     transfer_aic(ESCROW_ACCOUNT, job.provider, job.provider_bond)
//     
//     job.status = Completed
//     
//     // Update reputation
//     increment_provider_reputation(job.provider, job)
//
// fn slash_provider(job):
//     // Slash bond
//     slash_amount = job.provider_bond
//     burn_aic(slash_amount)
//     
//     // Refund requester
//     transfer_aic(ESCROW_ACCOUNT, job.requester, job.escrow_amount)
//     
//     job.status = Failed
//     
//     // Penalize reputation
//     decrement_provider_reputation(job.provider)
// ```
//
// VCR (Verifiable Compute Receipt):
// ```
// struct Vcr:
//     job_id: H256
//     provider: Address
//     model_hash: H256
//     code_hash: H256
//     input_hash: H256
//     output_hash: H256
//     seed: u64
//     tee_quote: Vec<u8>  // SEV-SNP/TDX attestation
//     kzg_commits: Option<Vec<KzgCommitment>>  // Trace commitments
//     signature: Signature
// ```
//
// PARAMETERS:
// - VCR_CHALLENGE_WINDOW: 1200 slots (~10 minutes)
// - VCR_BOND_MINIMUM: 10,000,000 AIC
// - BOND_RATIO: 0.1 (10% of job value)
//
// OUTPUTS:
// - AIC burn → Deflationary tokenomics
// - Provider payments → Incentivize AI inference
// - Reputation updates → Router optimization
// - Dispute resolution → Network security
// ============================================================================

pub mod job;
pub mod escrow;
pub mod vcr;
pub mod settlement;
pub mod dispute;

pub use job::Job;
pub use vcr::Vcr;

