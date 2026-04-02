//! Property-based tests for the Aether Rust SDK builders and types.
//!
//! Invariants verified:
//! - `TransferBuilder` rejects missing fields and accepts valid combinations.
//! - `JobBuilder` rejects empty job_id and missing required fields.
//! - `JobBuilder::max_fee` ignores zero values (preserves previous non-zero fee).
//! - `JobBuilder::to_submission` produces consistent URL from endpoint.
//! - `ClientConfig` default values are within expected ranges.
//! - `TransferRequest` serde roundtrip is deterministic.
//! - `JobRequest` serde roundtrip is deterministic.
//! - Built transactions have sender derived from keypair public key.
//! - Built transactions have a valid signature (verify_signature passes).
//! - Built transactions carry the nonce passed to `build`.
//! - Chain ID is preserved in the built transaction.

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use aether_crypto_primitives::Keypair;
    use aether_types::{Address, H256};

    use crate::job_builder::JobBuilder;
    use crate::transaction_builder::TransferBuilder;
    use crate::types::{ClientConfig, JobRequest, TransferRequest};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn arb_h256() -> impl Strategy<Value = H256> {
        prop::array::uniform32(any::<u8>()).prop_map(H256)
    }

    fn arb_address() -> impl Strategy<Value = Address> {
        prop::array::uniform20(any::<u8>()).prop_map(aether_types::H160)
    }

    fn arb_config() -> impl Strategy<Value = ClientConfig> {
        (1u128..10_000_000u128, 1u64..10_000_000u64).prop_map(|(fee, gas)| ClientConfig {
            default_fee: fee,
            default_gas_limit: gas,
        })
    }

    fn arb_nonempty_string() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_-]{0,31}".prop_map(|s| s)
    }

    // -----------------------------------------------------------------------
    // ClientConfig
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn config_default_in_range(_x in 0u8..1) {
            let cfg = ClientConfig::default();
            prop_assert!(cfg.default_fee > 0, "default_fee must be positive");
            prop_assert!(cfg.default_gas_limit > 0, "default_gas_limit must be positive");
        }

        #[test]
        fn config_serde_roundtrip(cfg in arb_config()) {
            let encoded = serde_json::to_string(&cfg).unwrap();
            let decoded: ClientConfig = serde_json::from_str(&encoded).unwrap();
            prop_assert_eq!(cfg.default_fee, decoded.default_fee);
            prop_assert_eq!(cfg.default_gas_limit, decoded.default_gas_limit);
        }
    }

    // -----------------------------------------------------------------------
    // TransferRequest serde roundtrip
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn transfer_request_serde_roundtrip(
            recipient in arb_address(),
            amount in any::<u128>(),
            memo in proptest::option::of("[a-zA-Z0-9 ]{0,64}"),
        ) {
            let req = TransferRequest { recipient, amount, memo };
            let encoded = bincode::serialize(&req).unwrap();
            let decoded: TransferRequest = bincode::deserialize(&encoded).unwrap();
            prop_assert_eq!(req.recipient, decoded.recipient);
            prop_assert_eq!(req.amount, decoded.amount);
            prop_assert_eq!(req.memo, decoded.memo);
        }
    }

    // -----------------------------------------------------------------------
    // JobRequest serde roundtrip
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn job_request_serde_roundtrip(
            job_id in arb_nonempty_string(),
            model_hash in arb_h256(),
            input_hash in arb_h256(),
            max_fee in 1u128..u128::MAX,
            expires_at in 1u64..u64::MAX,
        ) {
            let req = JobRequest {
                job_id,
                model_hash,
                input_hash,
                max_fee,
                expires_at,
                metadata: None,
            };
            let encoded = serde_json::to_string(&req).unwrap();
            let decoded: JobRequest = serde_json::from_str(&encoded).unwrap();
            prop_assert_eq!(req, decoded);
        }
    }

    // -----------------------------------------------------------------------
    // JobBuilder invariants
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn job_builder_rejects_empty_job_id(_x in 0u8..1) {
            let b = JobBuilder::new("http://localhost:8080");
            let result = b.job_id("");
            prop_assert!(result.is_err(), "empty job_id must be rejected");
        }

        #[test]
        fn job_builder_rejects_whitespace_job_id(spaces in "  +") {
            let b = JobBuilder::new("http://localhost:8080");
            let result = b.job_id(spaces);
            prop_assert!(result.is_err(), "whitespace-only job_id must be rejected");
        }

        #[test]
        fn job_builder_missing_field_returns_err(job_id in arb_nonempty_string()) {
            // Only job_id set — model_hash, input_hash, expires_at missing.
            let b = JobBuilder::new("http://localhost:8080")
                .job_id(job_id).unwrap();
            prop_assert!(b.build().is_err());
        }

        #[test]
        fn job_builder_zero_fee_ignored(
            job_id in arb_nonempty_string(),
            model_hash in arb_h256(),
            input_hash in arb_h256(),
            initial_fee in 1u128..1_000_000u128,
            expires_at in 1u64..u64::MAX,
        ) {
            let req = JobBuilder::new("http://localhost:8080")
                .job_id(job_id).unwrap()
                .model_hash(model_hash)
                .input_hash(input_hash)
                .max_fee(initial_fee)
                .max_fee(0)             // zero must be ignored
                .expires_at(expires_at)
                .build()
                .unwrap();
            prop_assert_eq!(req.max_fee, initial_fee,
                "zero fee must not overwrite a valid fee");
        }

        #[test]
        fn job_builder_max_fee_set(
            job_id in arb_nonempty_string(),
            model_hash in arb_h256(),
            input_hash in arb_h256(),
            max_fee in 1u128..1_000_000_000u128,
            expires_at in 1u64..u64::MAX,
        ) {
            let req = JobBuilder::new("http://localhost:8080")
                .job_id(job_id).unwrap()
                .model_hash(model_hash)
                .input_hash(input_hash)
                .max_fee(max_fee)
                .expires_at(expires_at)
                .build()
                .unwrap();
            prop_assert_eq!(req.max_fee, max_fee);
        }

        #[test]
        fn job_builder_submission_url_contains_endpoint(
            job_id in arb_nonempty_string(),
            model_hash in arb_h256(),
            input_hash in arb_h256(),
            expires_at in 1u64..u64::MAX,
        ) {
            let endpoint = "http://ai-mesh.internal:9000";
            let sub = JobBuilder::new(endpoint)
                .job_id(job_id).unwrap()
                .model_hash(model_hash)
                .input_hash(input_hash)
                .expires_at(expires_at)
                .to_submission()
                .unwrap();
            prop_assert!(sub.url.starts_with(endpoint),
                "submission URL must start with endpoint: {}", sub.url);
            prop_assert_eq!(sub.method, "POST");
        }

        #[test]
        fn job_builder_trailing_slash_stripped(
            job_id in arb_nonempty_string(),
            model_hash in arb_h256(),
            input_hash in arb_h256(),
            expires_at in 1u64..u64::MAX,
        ) {
            // Endpoint with trailing slash must produce the same URL as without.
            let sub_no_slash = JobBuilder::new("http://node:9000")
                .job_id(job_id.clone()).unwrap()
                .model_hash(model_hash)
                .input_hash(input_hash)
                .expires_at(expires_at)
                .to_submission()
                .unwrap();
            let sub_with_slash = JobBuilder::new("http://node:9000/")
                .job_id(job_id).unwrap()
                .model_hash(model_hash)
                .input_hash(input_hash)
                .expires_at(expires_at)
                .to_submission()
                .unwrap();
            prop_assert_eq!(sub_no_slash.url, sub_with_slash.url,
                "trailing slash must be stripped from endpoint");
        }
    }

    // -----------------------------------------------------------------------
    // TransferBuilder invariants
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn transfer_builder_missing_recipient_rejected(
            cfg in arb_config(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
        ) {
            let keypair = Keypair::generate();
            let result = TransferBuilder::new(&cfg)
                .amount(amount)
                .build(&keypair, nonce);
            prop_assert!(result.is_err(), "missing recipient must be rejected");
        }

        #[test]
        fn transfer_builder_missing_amount_rejected(
            cfg in arb_config(),
            recipient in arb_address(),
            nonce in any::<u64>(),
        ) {
            let keypair = Keypair::generate();
            let result = TransferBuilder::new(&cfg)
                .to(recipient)
                .build(&keypair, nonce);
            prop_assert!(result.is_err(), "missing amount must be rejected");
        }

        #[test]
        fn transfer_builder_valid_tx_has_correct_nonce(
            recipient in arb_address(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
        ) {
            // Use a fee well above the devnet minimum (~1,011,500 for default gas).
            let cfg = ClientConfig { default_fee: 5_000_000, default_gas_limit: 500_000 };
            let keypair = Keypair::generate();
            let tx = TransferBuilder::new(&cfg)
                .to(recipient)
                .amount(amount)
                .build(&keypair, nonce)
                .unwrap();
            prop_assert_eq!(tx.nonce, nonce, "nonce must be preserved in transaction");
        }

        #[test]
        fn transfer_builder_valid_tx_chain_id_preserved(
            recipient in arb_address(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
            chain_id in 1u64..1000u64,
        ) {
            let cfg = ClientConfig { default_fee: 5_000_000, default_gas_limit: 500_000 };
            let keypair = Keypair::generate();
            let tx = TransferBuilder::new(&cfg)
                .to(recipient)
                .amount(amount)
                .chain_id(chain_id)
                .build(&keypair, nonce)
                .unwrap();
            prop_assert_eq!(tx.chain_id, chain_id);
        }

        #[test]
        fn transfer_builder_signature_verifies(
            recipient in arb_address(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
        ) {
            let cfg = ClientConfig { default_fee: 5_000_000, default_gas_limit: 500_000 };
            let keypair = Keypair::generate();
            let tx = TransferBuilder::new(&cfg)
                .to(recipient)
                .amount(amount)
                .build(&keypair, nonce)
                .unwrap();
            prop_assert!(
                tx.verify_signature().is_ok(),
                "built transaction must have valid ed25519 signature"
            );
        }

        #[test]
        fn transfer_builder_sender_matches_keypair(
            recipient in arb_address(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
        ) {
            use aether_types::PublicKey;
            let cfg = ClientConfig { default_fee: 5_000_000, default_gas_limit: 500_000 };
            let keypair = Keypair::generate();
            let expected_address = PublicKey::from_bytes(keypair.public_key()).to_address();
            let tx = TransferBuilder::new(&cfg)
                .to(recipient)
                .amount(amount)
                .build(&keypair, nonce)
                .unwrap();
            prop_assert_eq!(tx.sender, expected_address,
                "sender must match keypair's derived address");
        }

        #[test]
        fn transfer_builder_fee_override_respected(
            recipient in arb_address(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
            // fee must exceed devnet minimum: a=10000 + b*~400bytes + c*500000 ≈ 1,012,000
            custom_fee in 2_000_000u128..10_000_000u128,
        ) {
            let cfg = ClientConfig { default_fee: 5_000_000, default_gas_limit: 500_000 };
            let keypair = Keypair::generate();
            let tx = TransferBuilder::new(&cfg)
                .to(recipient)
                .amount(amount)
                .fee(custom_fee)
                .build(&keypair, nonce)
                .unwrap();
            prop_assert_eq!(tx.fee, custom_fee, "explicit fee must be preserved");
        }

        #[test]
        fn transfer_builder_gas_limit_override_respected(
            recipient in arb_address(),
            amount in 1u128..1_000_000u128,
            nonce in any::<u64>(),
            // Keep gas low so fee stays above minimum: min = 10000 + 5*400 + 2*gas
            // With fee=5_000_000 and gas up to 100_000: min = 10000+2000+200000 = 212000 < 5M. OK.
            custom_gas in 1u64..100_000u64,
        ) {
            let cfg = ClientConfig { default_fee: 5_000_000, default_gas_limit: 500_000 };
            let keypair = Keypair::generate();
            let tx = TransferBuilder::new(&cfg)
                .to(recipient)
                .amount(amount)
                .gas_limit(custom_gas)
                .build(&keypair, nonce)
                .unwrap();
            prop_assert_eq!(tx.gas_limit, custom_gas, "explicit gas_limit must be preserved");
        }
    }
}
