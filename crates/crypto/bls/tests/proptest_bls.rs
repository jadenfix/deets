use aether_crypto_bls::aggregate::{aggregate_public_keys, aggregate_signatures};
use aether_crypto_bls::keypair::{verify, verify_pop, BlsKeypair};
use aether_crypto_bls::verify::{verify_aggregated, verify_aggregated_with_pop};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Sign-verify roundtrip: any message signed by a keypair must verify.
    #[test]
    fn sign_verify_roundtrip(msg in proptest::collection::vec(any::<u8>(), 0..512)) {
        let kp = BlsKeypair::generate();
        let sig = kp.sign(&msg);
        let verified = verify(&kp.public_key(), &msg, &sig).unwrap();
        prop_assert!(verified, "valid signature must verify");
    }

    /// Wrong key must not verify a valid signature.
    #[test]
    fn wrong_key_rejects(msg in proptest::collection::vec(any::<u8>(), 1..256)) {
        let kp1 = BlsKeypair::generate();
        let kp2 = BlsKeypair::generate();
        let sig = kp1.sign(&msg);
        let result = verify(&kp2.public_key(), &msg, &sig).unwrap();
        prop_assert!(!result, "wrong key must reject");
    }

    /// Wrong message must not verify a valid signature.
    #[test]
    fn wrong_message_rejects(
        msg1 in proptest::collection::vec(any::<u8>(), 1..256),
        msg2 in proptest::collection::vec(any::<u8>(), 1..256),
    ) {
        prop_assume!(msg1 != msg2);
        let kp = BlsKeypair::generate();
        let sig = kp.sign(&msg1);
        let result = verify(&kp.public_key(), &msg2, &sig).unwrap();
        prop_assert!(!result, "wrong message must reject");
    }

    /// Aggregate N signatures, verify against aggregated pubkey.
    #[test]
    fn aggregate_verify_roundtrip(n in 2usize..=8) {
        let msg = b"aggregate test";
        let keypairs: Vec<_> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let sigs: Vec<_> = keypairs.iter().map(|kp| kp.sign(msg)).collect();
        let pks: Vec<_> = keypairs.iter().map(|kp| kp.public_key()).collect();

        let agg_sig = aggregate_signatures(&sigs).unwrap();
        let agg_pk = aggregate_public_keys(&pks).unwrap();
        let verified = verify_aggregated(&agg_pk, msg, &agg_sig).unwrap();
        prop_assert!(verified, "aggregated signature must verify");
    }

    /// Aggregated signature must not verify with a missing signer's key.
    #[test]
    fn aggregate_missing_signer_rejects(n in 3usize..=6) {
        let msg = b"missing signer test";
        let keypairs: Vec<_> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let sigs: Vec<_> = keypairs.iter().map(|kp| kp.sign(msg)).collect();
        // Aggregate all sigs but only n-1 pubkeys
        let pks: Vec<_> = keypairs[..n-1].iter().map(|kp| kp.public_key()).collect();

        let agg_sig = aggregate_signatures(&sigs).unwrap();
        let agg_pk = aggregate_public_keys(&pks).unwrap();
        let verified = verify_aggregated(&agg_pk, msg, &agg_sig).unwrap();
        prop_assert!(!verified, "missing signer key must cause rejection");
    }

    /// Duplicate signatures must be rejected during aggregation.
    #[test]
    fn duplicate_signatures_rejected(n in 2usize..=5) {
        let msg = b"dup test";
        let keypairs: Vec<_> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let mut sigs: Vec<_> = keypairs.iter().map(|kp| kp.sign(msg)).collect();
        sigs.push(sigs[0].clone());
        let result = aggregate_signatures(&sigs);
        prop_assert!(result.is_err(), "duplicate signatures must be rejected");
    }

    /// PoP roundtrip: generated PoP must verify for same keypair.
    #[test]
    fn pop_roundtrip(_dummy in 0u8..10) {
        let kp = BlsKeypair::generate();
        let pop = kp.proof_of_possession();
        let valid = verify_pop(&kp.public_key(), &pop).unwrap();
        prop_assert!(valid, "PoP must verify for own keypair");
    }

    /// PoP cross-key rejection: PoP from one key must not verify with another.
    #[test]
    fn pop_cross_key_rejects(_dummy in 0u8..10) {
        let kp1 = BlsKeypair::generate();
        let kp2 = BlsKeypair::generate();
        let pop1 = kp1.proof_of_possession();
        let valid = verify_pop(&kp2.public_key(), &pop1).unwrap();
        prop_assert!(!valid, "PoP from different key must not verify");
    }

    /// verify_aggregated_with_pop end-to-end roundtrip.
    #[test]
    fn aggregate_with_pop_roundtrip(n in 2usize..=5) {
        let msg = b"pop aggregate test";
        let keypairs: Vec<_> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let sigs: Vec<_> = keypairs.iter().map(|kp| kp.sign(msg)).collect();
        let pks: Vec<_> = keypairs.iter().map(|kp| kp.public_key()).collect();
        let pops: Vec<_> = keypairs.iter().map(|kp| kp.proof_of_possession()).collect();

        let agg_sig = aggregate_signatures(&sigs).unwrap();
        let verified = verify_aggregated_with_pop(&pks, &pops, msg, &agg_sig).unwrap();
        prop_assert!(verified, "valid PoPs + valid aggregate must verify");
    }

    /// Invalid-length signatures must be rejected (not panic).
    #[test]
    fn invalid_length_sig_no_panic(len in 0usize..200) {
        prop_assume!(len != 96);
        let data = vec![0xABu8; len];
        let result = aggregate_signatures(&[data]);
        prop_assert!(result.is_err());
    }

    /// Invalid-length pubkeys must be rejected (not panic).
    #[test]
    fn invalid_length_pk_no_panic(len in 0usize..200) {
        prop_assume!(len != 48);
        let data = vec![0xCDu8; len];
        let result = aggregate_public_keys(&[data]);
        prop_assert!(result.is_err());
    }

    /// Signature is deterministic: same key + same message = same signature.
    #[test]
    fn signature_deterministic(msg in proptest::collection::vec(any::<u8>(), 0..256)) {
        let kp = BlsKeypair::generate();
        let s1 = kp.sign(&msg);
        let s2 = kp.sign(&msg);
        prop_assert_eq!(s1, s2, "BLS signing must be deterministic");
    }

    /// Random garbage as pubkey+sig must not panic — only return Err or Ok(false).
    #[test]
    fn garbage_verify_no_panic(
        pk in proptest::collection::vec(any::<u8>(), 0..128),
        msg in proptest::collection::vec(any::<u8>(), 0..64),
        sig in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let _ = verify(&pk, &msg, &sig);
    }

    /// Random garbage as aggregated pubkey+sig must not panic.
    #[test]
    fn garbage_verify_aggregated_no_panic(
        pk in proptest::collection::vec(any::<u8>(), 0..128),
        msg in proptest::collection::vec(any::<u8>(), 0..64),
        sig in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let _ = verify_aggregated(&pk, &msg, &sig);
    }

    /// Random garbage as PoP must not panic.
    #[test]
    fn garbage_pop_no_panic(
        pk in proptest::collection::vec(any::<u8>(), 0..128),
        pop in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let _ = verify_pop(&pk, &pop);
    }

    /// Random garbage bytes for aggregation must not panic.
    #[test]
    fn garbage_aggregate_sigs_no_panic(
        data in proptest::collection::vec(
            proptest::collection::vec(any::<u8>(), 0..128),
            0..5
        ),
    ) {
        let _ = aggregate_signatures(&data);
    }

    /// Random garbage bytes for pubkey aggregation must not panic.
    #[test]
    fn garbage_aggregate_pks_no_panic(
        data in proptest::collection::vec(
            proptest::collection::vec(any::<u8>(), 0..64),
            0..5
        ),
    ) {
        let _ = aggregate_public_keys(&data);
    }
}
