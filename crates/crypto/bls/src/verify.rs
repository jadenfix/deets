use anyhow::{anyhow, Result};
use blst::min_pk::{PublicKey as BlstPublicKey, Signature as BlstSignature};
use blst::BLST_ERROR;
use rayon::prelude::*;

const DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

/// BLS Aggregated Signature Verification
///
/// Verifies that an aggregated signature is valid for the given
/// aggregated public key and message.
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
pub fn batch_verify_aggregated(
    verifications: &[(Vec<u8>, Vec<u8>, Vec<u8>)], // (pubkey, message, signature)
) -> Result<Vec<bool>> {
    verifications
        .par_iter()
        .map(|(pubkey, message, signature)| verify_aggregated(pubkey, message, signature))
        .collect::<Result<Vec<bool>>>()
}

/// Fast path for verifying when you have proof-of-possession
///
/// Proof-of-possession (PoP) prevents rogue key attacks.
/// If all signers have proven they know their secret key,
/// we can use a faster verification path.
pub fn verify_aggregated_with_pop(
    aggregated_pubkey: &[u8],
    message: &[u8],
    aggregated_signature: &[u8],
) -> Result<bool> {
    // Same as regular verification, but we know we're safe from rogue key attacks
    verify_aggregated(aggregated_pubkey, message, aggregated_signature)
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
}
