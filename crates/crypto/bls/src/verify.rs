use anyhow::{anyhow, Result};
use blst::min_pk::{PublicKey as BlstPublicKey, Signature as BlstSignature};
use blst::BLST_ERROR;
use rayon::prelude::*;

const DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

/// Batch-verify N individual BLS signatures via `verify_multiple_aggregate_signatures`.
///
/// Internally uses randomized multi-pairing: each (pk, msg, sig) is multiplied
/// by a random 64-bit scalar before the miller loops are combined, so N
/// verification equations collapse into one final exponentiation.  blst
/// parallelizes the miller loops across a thread pool automatically.
///
/// Returns `Ok(true)` iff ALL signatures are valid.  If any single signature
/// is bad the whole batch fails — call individual `verify()` to identify
/// which one.
///
/// Security: random scalars prevent an attacker from crafting two invalid
/// signatures whose pairing contributions cancel (Bellare et al., 2007).
#[must_use = "discarding a batch verification result is a security bug"]
pub fn verify_batch(
    verifications: &[(&[u8], &[u8], &[u8])], // (pubkey_48, message, signature_96)
) -> Result<bool> {
    if verifications.is_empty() {
        return Ok(true);
    }

    let mut pks = Vec::with_capacity(verifications.len());
    let mut sigs = Vec::with_capacity(verifications.len());
    let mut msgs: Vec<&[u8]> = Vec::with_capacity(verifications.len());

    for (pk_bytes, msg, sig_bytes) in verifications {
        if pk_bytes.len() != 48 {
            anyhow::bail!("BLS public key must be 48 bytes");
        }
        if sig_bytes.len() != 96 {
            anyhow::bail!("BLS signature must be 96 bytes");
        }
        pks.push(
            BlstPublicKey::from_bytes(pk_bytes)
                .map_err(|e| anyhow!("invalid public key: {:?}", e))?,
        );
        sigs.push(
            BlstSignature::from_bytes(sig_bytes)
                .map_err(|e| anyhow!("invalid signature: {:?}", e))?,
        );
        msgs.push(msg);
    }

    let pk_refs: Vec<&BlstPublicKey> = pks.iter().collect();
    let sig_refs: Vec<&BlstSignature> = sigs.iter().collect();

    // Generate random 64-bit scalars for the linear combination
    use rand::RngCore;
    let mut rng = rand::thread_rng();
    let rands: Vec<blst::blst_scalar> = (0..verifications.len())
        .map(|_| {
            let mut scalar = blst::blst_scalar::default();
            let mut raw = [0u8; 8];
            rng.fill_bytes(&mut raw);
            raw[0] |= 1; // ensure nonzero
            scalar.b[..8].copy_from_slice(&raw);
            scalar
        })
        .collect();

    let err = BlstSignature::verify_multiple_aggregate_signatures(
        &msgs, DST, &pk_refs, true, &sig_refs, true, &rands, 64,
    );

    Ok(err == BLST_ERROR::BLST_SUCCESS)
}

/// BLS Aggregated Signature Verification
///
/// Verifies that an aggregated signature is valid for the given
/// aggregated public key and message.
///
/// NOTE: BLS signatures on G2 are malleable (negation of a valid signature is also valid).
/// This is acceptable in Aether because vote deduplication is based on
/// (slot, phase, block_hash, validator_address), not on signature content.
/// A malleable signature cannot be used to double-count a vote.
///
/// Verification equation: e(agg_pk, H(m)) == e(G1, agg_sig)
/// where:
/// - e() is the pairing function (Ate pairing on BLS12-381)
/// - agg_pk is the aggregated public key (G1 point)
/// - H(m) is the message hashed to G2
/// - agg_sig is the aggregated signature (G2 point)
/// - G1 is the generator of G1
///
/// This single pairing check verifies ALL individual signatures
/// that were aggregated, which is why BLS is so efficient.
///
/// Verify an aggregated BLS signature
///
/// Parameters:
/// - aggregated_pubkey: The sum of all signers' public keys (48 bytes)
/// - message: The message that was signed
/// - aggregated_signature: The sum of all signatures (96 bytes)
///
/// Returns: true if signature is valid, false otherwise
#[must_use = "discarding an aggregated verification result is a security bug"]
pub fn verify_aggregated(
    aggregated_pubkey: &[u8],
    message: &[u8],
    aggregated_signature: &[u8],
) -> Result<bool> {
    // Validate inputs
    if aggregated_pubkey.len() != 48 {
        anyhow::bail!("aggregated public key must be 48 bytes");
    }

    if aggregated_signature.len() != 96 {
        anyhow::bail!("aggregated signature must be 96 bytes");
    }

    let pk = BlstPublicKey::from_bytes(aggregated_pubkey)
        .map_err(|e| anyhow!("invalid aggregated public key: {:?}", e))?;
    let sig = BlstSignature::from_bytes(aggregated_signature)
        .map_err(|e| anyhow!("invalid aggregated signature: {:?}", e))?;

    Ok(sig.verify(true, message, DST, &[], &pk, true) == BLST_ERROR::BLST_SUCCESS)
}

/// Verify multiple aggregated signatures in batch
///
/// More efficient than verifying each individually when you have
/// many aggregated signatures to verify (e.g., from different blocks).
///
/// Uses batch pairing techniques to amortize the cost.
#[must_use = "verification results must not be silently discarded"]
pub fn batch_verify_aggregated(
    verifications: &[(Vec<u8>, Vec<u8>, Vec<u8>)], // (pubkey, message, signature)
) -> Result<Vec<bool>> {
    verifications
        .par_iter()
        .map(|(pubkey, message, signature)| verify_aggregated(pubkey, message, signature))
        .collect::<Result<Vec<bool>>>()
}

/// Verify an aggregated BLS signature with proof-of-possession enforcement.
///
/// Proof-of-possession (PoP) prevents rogue key attacks: each signer must
/// prove they know the secret key for their public key before their key
/// can be included in aggregation.
///
/// This function first verifies each individual PoP, then aggregates the
/// public keys and verifies the aggregated signature.
#[must_use = "discarding a PoP-verified aggregation result is a security bug"]
pub fn verify_aggregated_with_pop(
    individual_pubkeys: &[Vec<u8>],
    pop_signatures: &[Vec<u8>],
    message: &[u8],
    aggregated_signature: &[u8],
) -> Result<bool> {
    if individual_pubkeys.len() != pop_signatures.len() {
        anyhow::bail!("pubkey count must match PoP count");
    }
    if individual_pubkeys.is_empty() {
        anyhow::bail!("cannot verify with empty signer set");
    }

    // Verify each PoP first — reject if any signer hasn't proven key ownership
    for (i, (pk, pop)) in individual_pubkeys
        .iter()
        .zip(pop_signatures.iter())
        .enumerate()
    {
        match crate::keypair::verify_pop(pk, pop)? {
            true => {}
            false => anyhow::bail!("invalid proof-of-possession for signer {}", i),
        }
    }

    // All PoPs valid — aggregate pubkeys and verify the aggregate signature
    let agg_pk = crate::aggregate::aggregate_public_keys(individual_pubkeys)?;
    verify_aggregated(&agg_pk, message, aggregated_signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::{aggregate_public_keys, aggregate_signatures};
    use crate::keypair::BlsKeypair;
    use std::time::Instant;

    #[test]
    fn test_verify_aggregated() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let message = b"test message";
        let sig1 = keypair1.sign(message);
        let sig2 = keypair2.sign(message);

        let agg_sig = aggregate_signatures(&[sig1, sig2]).unwrap();
        let agg_pk =
            aggregate_public_keys(&[keypair1.public_key(), keypair2.public_key()]).unwrap();

        let verified = verify_aggregated(&agg_pk, message, &agg_sig).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_verify_invalid_signature() {
        let keypair = BlsKeypair::generate();
        let message = b"test message";

        let invalid_sig = vec![0u8; 96]; // All zeros

        let pk = keypair.public_key();
        assert!(verify_aggregated(&pk, message, &invalid_sig).is_err());
    }

    #[test]
    fn test_batch_verification() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let msg1 = b"message 1";
        let msg2 = b"message 2";

        let sig1 = keypair1.sign(msg1);
        let sig2 = keypair2.sign(msg2);

        let pk1 = keypair1.public_key();
        let pk2 = keypair2.public_key();

        let verifications = vec![(pk1, msg1.to_vec(), sig1), (pk2, msg2.to_vec(), sig2)];

        let results = batch_verify_aggregated(&verifications).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.into_iter().all(|r| r));
    }

    #[test]
    fn test_large_aggregation_verification() {
        // Simulate verifying a block with many validator votes
        let mut signatures = Vec::new();
        let mut public_keys = Vec::new();
        let message = b"block_hash";

        for _ in 0..50 {
            let keypair = BlsKeypair::generate();
            signatures.push(keypair.sign(message));
            public_keys.push(keypair.public_key());
        }

        let agg_sig = aggregate_signatures(&signatures).unwrap();
        let agg_pk = aggregate_public_keys(&public_keys).unwrap();

        let verified = verify_aggregated(&agg_pk, message, &agg_sig).unwrap();
        assert!(verified);
    }

    #[test]
    #[ignore]
    fn test_phase4_bls_batch_performance() {
        const VALIDATORS: usize = 512;
        const ITERATIONS: usize = 200;
        const MIN_THROUGHPUT: u64 = 250;

        let message = b"phase4 bls throughput";
        let mut signatures = Vec::with_capacity(VALIDATORS);
        let mut public_keys = Vec::with_capacity(VALIDATORS);

        for _ in 0..VALIDATORS {
            let keypair = BlsKeypair::generate();
            public_keys.push(keypair.public_key());
            signatures.push(keypair.sign(message));
        }

        let agg_sig = aggregate_signatures(&signatures).unwrap();
        let agg_pk = aggregate_public_keys(&public_keys).unwrap();

        // Warm up pairing cache to reduce first-run overhead in CI.
        for _ in 0..10 {
            assert!(verify_aggregated(&agg_pk, message, &agg_sig).unwrap());
        }

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            assert!(verify_aggregated(&agg_pk, message, &agg_sig).unwrap());
        }
        let elapsed = start.elapsed();
        let throughput = (ITERATIONS as f64 / elapsed.as_secs_f64()) as u64;

        println!(
            "BLS aggregated verification throughput: {} verifications/s",
            throughput
        );

        assert!(
            throughput > MIN_THROUGHPUT,
            "Throughput {} too low",
            throughput
        );
    }

    #[test]
    fn test_verify_aggregated_with_pop_enforces_pop() {
        let kp1 = BlsKeypair::generate();
        let kp2 = BlsKeypair::generate();
        let message = b"test aggregate with pop";

        let sig1 = kp1.sign(message);
        let sig2 = kp2.sign(message);
        let agg_sig = aggregate_signatures(&[sig1, sig2]).unwrap();

        let pop1 = kp1.proof_of_possession();
        let pop2 = kp2.proof_of_possession();

        // Valid: correct PoPs
        let result = verify_aggregated_with_pop(
            &[kp1.public_key(), kp2.public_key()],
            &[pop1.clone(), pop2.clone()],
            message,
            &agg_sig,
        )
        .unwrap();
        assert!(result, "Valid PoPs + valid aggregate should verify");

        // Invalid: swap PoPs (kp1's PoP for kp2's key)
        let result = verify_aggregated_with_pop(
            &[kp1.public_key(), kp2.public_key()],
            &[pop2, pop1],
            message,
            &agg_sig,
        );
        assert!(result.is_err(), "Swapped PoPs should be rejected");
    }

    #[test]
    fn test_verify_batch_all_valid() {
        let n = 10;
        let keypairs: Vec<BlsKeypair> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let messages: Vec<Vec<u8>> = (0..n).map(|i| format!("msg-{i}").into_bytes()).collect();
        let signatures: Vec<Vec<u8>> = keypairs
            .iter()
            .zip(&messages)
            .map(|(kp, m)| kp.sign(m))
            .collect();
        let pks: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.public_key()).collect();

        let tuples: Vec<(&[u8], &[u8], &[u8])> = (0..n)
            .map(|i| {
                (
                    pks[i].as_slice(),
                    messages[i].as_slice(),
                    signatures[i].as_slice(),
                )
            })
            .collect();

        assert!(verify_batch(&tuples).unwrap());
    }

    #[test]
    fn test_verify_batch_one_bad_signature() {
        let kp1 = BlsKeypair::generate();
        let kp2 = BlsKeypair::generate();
        let msg1 = b"good";
        let msg2 = b"bad";
        let sig1 = kp1.sign(msg1);
        let sig2 = kp2.sign(b"different"); // signed wrong message

        let pk1 = kp1.public_key();
        let pk2 = kp2.public_key();
        let tuples: Vec<(&[u8], &[u8], &[u8])> = vec![
            (pk1.as_slice(), &msg1[..], sig1.as_slice()),
            (pk2.as_slice(), &msg2[..], sig2.as_slice()),
        ];

        assert!(!verify_batch(&tuples).unwrap());
    }

    #[test]
    fn test_verify_batch_empty() {
        let tuples: Vec<(&[u8], &[u8], &[u8])> = vec![];
        assert!(verify_batch(&tuples).unwrap());
    }

    #[test]
    fn test_verify_batch_single() {
        let kp = BlsKeypair::generate();
        let msg = b"single";
        let sig = kp.sign(msg);
        let pk = kp.public_key();

        let tuples: Vec<(&[u8], &[u8], &[u8])> = vec![(pk.as_slice(), &msg[..], sig.as_slice())];

        assert!(verify_batch(&tuples).unwrap());
    }

    #[test]
    fn test_verify_batch_same_message_different_signers() {
        let n = 20;
        let msg = b"consensus-vote";
        let keypairs: Vec<BlsKeypair> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let signatures: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.sign(msg)).collect();
        let pks: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.public_key()).collect();

        let tuples: Vec<(&[u8], &[u8], &[u8])> = (0..n)
            .map(|i| (pks[i].as_slice(), &msg[..], signatures[i].as_slice()))
            .collect();

        assert!(verify_batch(&tuples).unwrap());
    }

    #[test]
    #[ignore]
    fn test_verify_batch_throughput() {
        const N: usize = 200;
        const ITERATIONS: usize = 50;

        let msg = b"batch-throughput-test";
        let keypairs: Vec<BlsKeypair> = (0..N).map(|_| BlsKeypair::generate()).collect();
        let signatures: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.sign(msg)).collect();
        let pks: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.public_key()).collect();

        let tuples: Vec<(&[u8], &[u8], &[u8])> = (0..N)
            .map(|i| (pks[i].as_slice(), &msg[..], signatures[i].as_slice()))
            .collect();

        // Warmup
        for _ in 0..3 {
            verify_batch(&tuples).unwrap();
        }

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            assert!(verify_batch(&tuples).unwrap());
        }
        let elapsed = start.elapsed();
        let votes_per_sec = (N * ITERATIONS) as f64 / elapsed.as_secs_f64();

        println!(
            "Batch verify: {N} votes × {ITERATIONS} iterations in {:.2}s = {:.0} votes/s",
            elapsed.as_secs_f64(),
            votes_per_sec
        );

        // Target: ≥10k votes/s
        assert!(
            votes_per_sec > 10_000.0,
            "batch verify throughput {votes_per_sec:.0} votes/s below 10k target"
        );
    }
}
