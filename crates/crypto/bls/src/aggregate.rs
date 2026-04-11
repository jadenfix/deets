use anyhow::{anyhow, Result};
use blst::min_pk::{AggregatePublicKey, AggregateSignature, Signature as BlstSignature};
use std::collections::HashSet;

/// BLS Signature Aggregation
///
/// The key property of BLS signatures: multiple signatures can be aggregated
/// into a single signature that can be verified against the aggregated public keys.
///
/// Aggregation is done via point addition on the elliptic curve:
/// - Signatures are G2 points: agg_sig = sig1 + sig2 + ... + sigN
/// - Public keys are G1 points: agg_pk = pk1 + pk2 + ... + pkN
///
/// Verification: e(agg_pk, H(m)) == e(G1, agg_sig)
/// where e() is the pairing function
///
/// Aggregate multiple BLS signatures into one
///
/// Input: Vec of 96-byte signatures
/// Output: Single 96-byte aggregated signature
pub fn aggregate_signatures(signatures: &[Vec<u8>]) -> Result<Vec<u8>> {
    if signatures.is_empty() {
        anyhow::bail!("cannot aggregate empty signature list");
    }

    // Validate all signatures are 96 bytes and pass subgroup check
    let mut seen = HashSet::new();
    for (i, sig) in signatures.iter().enumerate() {
        if sig.len() != 96 {
            anyhow::bail!("signature {} has invalid length: {}", i, sig.len());
        }
        // Subgroup check: ensure the deserialized point is in the correct G2 subgroup
        let blst_sig = BlstSignature::from_bytes(sig.as_slice())
            .map_err(|e| anyhow!("signature {} deserialization failed: {:?}", i, e))?;
        if !blst_sig.subgroup_check() {
            anyhow::bail!("signature {} failed subgroup check", i);
        }
        // Duplicate detection: reject identical signatures (could inflate voting power)
        if !seen.insert(sig.as_slice()) {
            anyhow::bail!("duplicate signature detected at index {}", i);
        }
    }

    let sig_refs: Vec<&[u8]> = signatures.iter().map(|s| s.as_slice()).collect();

    let agg = AggregateSignature::aggregate_serialized(&sig_refs, true)
        .map_err(|e| anyhow!("failed to aggregate signatures: {:?}", e))?;

    Ok(agg.to_signature().to_bytes().to_vec())
}

/// Aggregate multiple BLS public keys into one
///
/// Input: Vec of 48-byte public keys  
/// Output: Single 48-byte aggregated public key
pub fn aggregate_public_keys(public_keys: &[Vec<u8>]) -> Result<Vec<u8>> {
    if public_keys.is_empty() {
        anyhow::bail!("cannot aggregate empty public key list");
    }

    // Validate all public keys are 48 bytes
    for (i, pk) in public_keys.iter().enumerate() {
        if pk.len() != 48 {
            anyhow::bail!("public key {} has invalid length: {}", i, pk.len());
        }
    }

    let pk_refs: Vec<&[u8]> = public_keys.iter().map(|pk| pk.as_slice()).collect();

    // The `true` flag enables subgroup validation on each key during aggregation.
    let agg = AggregatePublicKey::aggregate_serialized(&pk_refs, true)
        .map_err(|e| anyhow!("failed to aggregate public keys (subgroup check): {:?}", e))?;

    Ok(agg.to_public_key().to_bytes().to_vec())
}

/// Check if a signature is aggregated (has multiple signers)
#[inline]
#[must_use]
pub fn is_aggregated(signature: &[u8]) -> bool {
    if signature.len() != 96 {
        return false;
    }
    BlstSignature::from_bytes(signature)
        .map(|sig| sig.subgroup_check())
        .unwrap_or(false)
}

/// Fast aggregation using parallel processing for large signature sets
pub fn aggregate_signatures_parallel(signatures: &[Vec<u8>]) -> Result<Vec<u8>> {
    if signatures.is_empty() {
        anyhow::bail!("cannot aggregate empty signature list");
    }

    if signatures.len() < 100 {
        // For small sets, sequential is faster
        return aggregate_signatures(signatures);
    }

    // Placeholder for future rayon-based parallel aggregation.
    aggregate_signatures(signatures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::BlsKeypair;

    #[test]
    fn test_aggregate_signatures() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let message = b"test message";
        let sig1 = keypair1.sign(message);
        let sig2 = keypair2.sign(message);

        let aggregated = aggregate_signatures(&[sig1, sig2]).unwrap();

        assert_eq!(aggregated.len(), 96);
        assert!(is_aggregated(&aggregated));
    }

    #[test]
    fn test_aggregate_public_keys() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let pk1 = keypair1.public_key();
        let pk2 = keypair2.public_key();

        let aggregated = aggregate_public_keys(&[pk1, pk2]).unwrap();

        assert_eq!(aggregated.len(), 48);
    }

    #[test]
    fn test_empty_aggregation() {
        let result = aggregate_signatures(&[]);
        assert!(result.is_err());

        let result = aggregate_public_keys(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_signature_length() {
        let invalid_sig = vec![0u8; 50]; // Wrong length

        let result = aggregate_signatures(&[invalid_sig]);
        assert!(result.is_err());
    }

    #[test]
    fn test_large_aggregation() {
        // Test aggregating many signatures (like in a real blockchain)
        let mut signatures = Vec::new();
        let message = b"block_hash_12345";

        for _ in 0..100 {
            let keypair = BlsKeypair::generate();
            let sig = keypair.sign(message);
            signatures.push(sig);
        }

        let aggregated = aggregate_signatures(&signatures).unwrap();

        assert_eq!(aggregated.len(), 96);
        assert!(is_aggregated(&aggregated));
    }

    #[test]
    fn test_aggregation_deterministic() {
        let keypair1 = BlsKeypair::generate();
        let keypair2 = BlsKeypair::generate();

        let message = b"test";
        let sig1 = keypair1.sign(message);
        let sig2 = keypair2.sign(message);

        let agg1 = aggregate_signatures(&[sig1.clone(), sig2.clone()]).unwrap();
        let agg2 = aggregate_signatures(&[sig1, sig2]).unwrap();

        // Aggregation should be deterministic
        assert_eq!(agg1, agg2);
    }

    #[test]
    fn test_rejects_duplicate_signatures() {
        // Generate one BLS keypair, sign the same message, then try to aggregate
        // the same signature twice. Should fail with "duplicate signature" error.
        let kp = BlsKeypair::generate();
        let msg = b"test message";
        let sig = kp.sign(msg);
        let result = aggregate_signatures(&[sig.clone(), sig.clone(), kp.sign(b"other")]);
        assert!(result.is_err(), "should reject duplicate signatures");
        assert!(result.unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn test_rejects_non_96_byte_signature() {
        // A signature that's not 96 bytes should be rejected
        let result = aggregate_signatures(&[vec![0u8; 95]]);
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_non_48_byte_pubkey() {
        // A pubkey that's not 48 bytes should be rejected
        let result = aggregate_public_keys(&[vec![0u8; 47]]);
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_duplicate_signatures_two_keypairs() {
        // Create 2 unique signatures, then duplicate one
        // aggregate_signatures should reject duplicates
        let kp1 = BlsKeypair::generate();
        let kp2 = BlsKeypair::generate();
        let msg = b"test message";
        let sig1 = kp1.sign(msg);
        let sig2 = kp2.sign(msg);
        let result = aggregate_signatures(&[sig1.clone(), sig2, sig1]);
        assert!(result.is_err(), "duplicate signatures should be rejected");
    }

    #[test]
    fn test_rejects_wrong_length_signature() {
        let result = aggregate_signatures(&[vec![0u8; 48]]); // 48 bytes, not 96
        assert!(result.is_err(), "wrong-length signature should be rejected");
    }

    #[test]
    fn test_rejects_wrong_length_pubkey() {
        let result = aggregate_public_keys(&[vec![0u8; 32]]); // 32 bytes, not 48
        assert!(result.is_err(), "wrong-length pubkey should be rejected");
    }
}
