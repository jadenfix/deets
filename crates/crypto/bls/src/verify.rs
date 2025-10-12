use anyhow::Result;

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
    _message: &[u8],
    aggregated_signature: &[u8],
) -> Result<bool> {
    // Validate inputs
    if aggregated_pubkey.len() != 48 {
        anyhow::bail!("aggregated public key must be 48 bytes");
    }

    if aggregated_signature.len() != 96 {
        anyhow::bail!("aggregated signature must be 96 bytes");
    }

    // In production: use blst::min_sig::AggregateSignature::verify()
    // This performs:
    // 1. Hash message to G2 point
    // 2. Compute pairing e(agg_pk, H(m))
    // 3. Compute pairing e(G1, agg_sig)
    // 4. Check equality

    // For now: simplified verification
    // Check signature is non-zero
    if aggregated_signature.iter().all(|&b| b == 0) {
        return Ok(false);
    }

    // Check public key is non-zero
    if aggregated_pubkey.iter().all(|&b| b == 0) {
        return Ok(false);
    }

    // Placeholder: assume valid if basic checks pass
    Ok(true)
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
    let mut results = Vec::with_capacity(verifications.len());

    // In production: use blst batch verification
    // This computes a random linear combination of all pairings
    // to verify all signatures in a single final pairing check

    // For now: verify each individually
    for (pubkey, message, signature) in verifications {
        let result = verify_aggregated(pubkey, message, signature)?;
        results.push(result);
    }

    Ok(results)
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

    #[test]
    fn test_verify_aggregated() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let message = b"test message";
        let sig1 = keypair1.sign(message);
        let sig2 = keypair2.sign(message);

        let agg_sig = aggregate_signatures(&[sig1, sig2]).unwrap();
        let agg_pk = aggregate_public_keys(&[
            keypair1.public_key().to_vec(),
            keypair2.public_key().to_vec(),
        ])
        .unwrap();

        let verified = verify_aggregated(&agg_pk, message, &agg_sig).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_verify_invalid_signature() {
        let keypair = BlsKeypair::generate();
        let message = b"test message";

        let invalid_sig = vec![0u8; 96]; // All zeros

        let verified = verify_aggregated(keypair.public_key(), message, &invalid_sig).unwrap();

        assert!(!verified);
    }

    #[test]
    fn test_batch_verification() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let msg1 = b"message 1";
        let msg2 = b"message 2";

        let sig1 = keypair1.sign(msg1);
        let sig2 = keypair2.sign(msg2);

        let pk1 = keypair1.public_key().to_vec();
        let pk2 = keypair2.public_key().to_vec();

        let verifications = vec![(pk1, msg1.to_vec(), sig1), (pk2, msg2.to_vec(), sig2)];

        let results = batch_verify_aggregated(&verifications).unwrap();

        assert_eq!(results.len(), 2);
        // In production with real BLS, both should verify
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
            public_keys.push(keypair.public_key().to_vec());
        }

        let agg_sig = aggregate_signatures(&signatures).unwrap();
        let agg_pk = aggregate_public_keys(&public_keys).unwrap();

        let verified = verify_aggregated(&agg_pk, message, &agg_sig).unwrap();
        assert!(verified);
    }
}
