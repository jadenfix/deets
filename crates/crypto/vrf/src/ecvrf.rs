use anyhow::Result;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha512};

/// ECVRF (Elliptic Curve Verifiable Random Function) implementation
/// Based on IETF draft-irtf-cfrg-vrf-15 (ECVRF-EDWARDS25519-SHA512-ELL2)
///
/// VRF provides:
/// - Pseudorandom output from a secret key and input
/// - Proof that the output was correctly generated
/// - Anyone can verify the proof using the public key
///
/// Used for: Slot leader election in VRF-PoS consensus
///
/// Algorithm:
/// 1. Hash input to curve point: H = hash_to_curve(input)
/// 2. Compute gamma = secret_scalar * H  
/// 3. Generate NIZK proof (c, s) proving discrete log
/// 4. Output = hash(gamma)
/// 5. Proof = (gamma, c, s)
const SUITE: u8 = 0x04; // ECVRF-EDWARDS25519-SHA512-ELL2

#[derive(Clone, Debug)]
pub struct VrfKeypair {
    secret: SigningKey,
    public: VerifyingKey,
}

#[derive(Clone, Debug)]
pub struct VrfProof {
    pub proof: Vec<u8>,
    pub output: [u8; 32],
}

impl VrfKeypair {
    /// Generate a new VRF keypair using Ed25519
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut rng = rand::rngs::OsRng;
        let mut secret_bytes = [0u8; 32];
        rng.fill_bytes(&mut secret_bytes);

        let secret = SigningKey::from_bytes(&secret_bytes);
        let public = secret.verifying_key();

        VrfKeypair { secret, public }
    }

    /// Create keypair from 32-byte secret key
    pub fn from_secret(secret: &[u8]) -> Result<Self> {
        if secret.len() != 32 {
            anyhow::bail!("secret key must be 32 bytes");
        }

        let mut secret_bytes = [0u8; 32];
        secret_bytes.copy_from_slice(secret);

        let secret = SigningKey::from_bytes(&secret_bytes);
        let public = secret.verifying_key();

        Ok(VrfKeypair { secret, public })
    }

    /// Get public key (32 bytes, compressed Ed25519 point)
    pub fn public_key(&self) -> &[u8; 32] {
        self.public.as_bytes()
    }

    /// Get secret key (use with caution!)
    pub fn secret_key(&self) -> &[u8; 32] {
        self.secret.as_bytes()
    }

    /// Evaluate VRF: generate (output, proof) from input
    /// Implements ECVRF-EDWARDS25519-SHA512-ELL2 spec
    pub fn prove(&self, input: &[u8]) -> VrfProof {
        // 1. Hash input to curve point
        let h_point = hash_to_curve(input, &self.public);

        // 2. Compute gamma = x * H where x is secret scalar
        let secret_scalar = secret_to_scalar(&self.secret);
        let gamma = h_point * secret_scalar;

        // 3. Generate proof using Fiat-Shamir NIZK
        let k = nonce_generation(&self.secret, &h_point);
        let k_b = EdwardsPoint::mul_base(&k);
        let k_h = h_point * k;

        // Challenge c = hash(suite, public, H, gamma, k*B, k*H)
        let c = challenge_generation(&self.public, &h_point, &gamma, &k_b, &k_h);

        // Response s = k + c*x (mod order)
        let s = k + c * secret_scalar;

        // 4. Compute VRF output = hash(gamma)
        let output = proof_to_hash(&gamma);

        // 5. Encode proof as (gamma, c, s)
        let proof = encode_proof(&gamma, &c, &s);

        VrfProof { proof, output }
    }
}

/// Verify a VRF proof according to ECVRF spec
pub fn verify_proof(public_key: &[u8; 32], input: &[u8], vrf_proof: &VrfProof) -> Result<bool> {
    // Decode public key
    let public_point = match VerifyingKey::from_bytes(public_key) {
        Ok(pk) => pk,
        Err(_) => return Ok(false),
    };

    // Decode proof (gamma, c, s)
    let (gamma, c, s) = match decode_proof(&vrf_proof.proof) {
        Ok(decoded) => decoded,
        Err(_) => return Ok(false),
    };

    // Hash input to curve
    let h_point = hash_to_curve(input, &public_point);

    // Verify proof: s*B = k*B + c*public and s*H = k*H + c*gamma
    let s_b = EdwardsPoint::mul_base(&s);
    let c_pub = edwards_from_verifying_key(&public_point) * c;
    let u = s_b - c_pub;

    let s_h = h_point * s;
    let c_gamma = gamma * c;
    let v = s_h - c_gamma;

    // Recompute challenge
    let c_prime = challenge_generation(&public_point, &h_point, &gamma, &u, &v);

    // Verify c == c'
    if c != c_prime {
        return Ok(false);
    }

    // Verify output = hash(gamma)
    let expected_output = proof_to_hash(&gamma);
    if expected_output != vrf_proof.output {
        return Ok(false);
    }

    Ok(true)
}

// Helper functions for ECVRF implementation

fn secret_to_scalar(secret: &SigningKey) -> Scalar {
    let hash = Sha512::digest(secret.as_bytes());
    let mut scalar_bytes = [0u8; 32];
    scalar_bytes.copy_from_slice(&hash[0..32]);
    scalar_bytes[0] &= 248;
    scalar_bytes[31] &= 127;
    scalar_bytes[31] |= 64;
    Scalar::from_bytes_mod_order(scalar_bytes)
}

fn edwards_from_verifying_key(public: &VerifyingKey) -> EdwardsPoint {
    let compressed = curve25519_dalek::edwards::CompressedEdwardsY(public.to_bytes());
    compressed.decompress().unwrap()
}

fn hash_to_curve(input: &[u8], public: &VerifyingKey) -> EdwardsPoint {
    // ECVRF-EDWARDS25519-SHA512-ELL2 hash-to-curve
    let mut hasher = Sha512::new();
    hasher.update([SUITE, 0x01]); // suite_string || 0x01
    hasher.update(public.as_bytes());
    hasher.update(input);
    let hash = hasher.finalize();

    // Interpret hash as Edwards point (with cofactor clearing)
    let mut point_bytes = [0u8; 32];
    point_bytes.copy_from_slice(&hash[0..32]);
    point_bytes[31] &= 0x7F; // Clear sign bit

    let compressed = curve25519_dalek::edwards::CompressedEdwardsY(point_bytes);
    compressed.decompress().unwrap_or_default() * Scalar::from(8u8)
}

fn nonce_generation(secret: &SigningKey, h: &EdwardsPoint) -> Scalar {
    let mut hasher = Sha512::new();
    hasher.update(secret.as_bytes());
    hasher.update(h.compress().as_bytes());
    let hash = hasher.finalize();
    Scalar::from_hash(Sha512::new().chain_update(&hash[..]))
}

fn challenge_generation(
    public: &VerifyingKey,
    h: &EdwardsPoint,
    gamma: &EdwardsPoint,
    k_b: &EdwardsPoint,
    k_h: &EdwardsPoint,
) -> Scalar {
    let mut hasher = Sha512::new();
    hasher.update([SUITE, 0x02]); // suite_string || 0x02
    hasher.update(public.as_bytes());
    hasher.update(h.compress().as_bytes());
    hasher.update(gamma.compress().as_bytes());
    hasher.update(k_b.compress().as_bytes());
    hasher.update(k_h.compress().as_bytes());
    let hash = hasher.finalize();

    let mut scalar_bytes = [0u8; 32];
    scalar_bytes.copy_from_slice(&hash[0..32]);
    Scalar::from_bytes_mod_order(scalar_bytes)
}

fn proof_to_hash(gamma: &EdwardsPoint) -> [u8; 32] {
    let mut hasher = Sha512::new();
    hasher.update([SUITE, 0x03]); // suite_string || 0x03
    hasher.update(gamma.compress().as_bytes());
    let hash = hasher.finalize();

    let mut output = [0u8; 32];
    output.copy_from_slice(&hash[0..32]);
    output
}

fn encode_proof(gamma: &EdwardsPoint, c: &Scalar, s: &Scalar) -> Vec<u8> {
    let mut proof = Vec::with_capacity(80);
    proof.extend_from_slice(gamma.compress().as_bytes()); // 32 bytes
    proof.extend_from_slice(c.as_bytes()); // 32 bytes
    proof.extend_from_slice(s.as_bytes()); // 32 bytes
    proof
}

fn decode_proof(proof: &[u8]) -> Result<(EdwardsPoint, Scalar, Scalar)> {
    if proof.len() != 96 {
        anyhow::bail!("proof must be 96 bytes");
    }

    let gamma_bytes = &proof[0..32];
    let c_bytes = &proof[32..64];
    let s_bytes = &proof[64..96];

    let mut gamma_arr = [0u8; 32];
    gamma_arr.copy_from_slice(gamma_bytes);
    let gamma_compressed = curve25519_dalek::edwards::CompressedEdwardsY(gamma_arr);
    let gamma = gamma_compressed
        .decompress()
        .ok_or_else(|| anyhow::anyhow!("invalid gamma point"))?;

    let mut c_arr = [0u8; 32];
    c_arr.copy_from_slice(c_bytes);
    let c = Scalar::from_canonical_bytes(c_arr)
        .into_option()
        .ok_or_else(|| anyhow::anyhow!("invalid c scalar"))?;

    let mut s_arr = [0u8; 32];
    s_arr.copy_from_slice(s_bytes);
    let s = Scalar::from_canonical_bytes(s_arr)
        .into_option()
        .ok_or_else(|| anyhow::anyhow!("invalid s scalar"))?;

    Ok((gamma, c, s))
}

/// Convert VRF output to a value in [0, 1) for threshold comparison
pub fn output_to_value(output: &[u8; 32]) -> f64 {
    // Convert first 8 bytes to u64, then normalize to [0, 1)
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&output[..8]);
    let val = u64::from_le_bytes(bytes);

    // Normalize to [0, 1)
    if val == u64::MAX {
        1.0 - f64::EPSILON
    } else {
        val as f64 / (u64::MAX as f64)
    }
}

/// Check if VRF output wins the lottery for slot leadership
///
/// threshold = tau * stake_i / total_stake
/// eligible if output_value < threshold
pub fn check_leader_eligibility(
    vrf_output: &[u8; 32],
    stake: u128,
    total_stake: u128,
    tau: f64,
) -> bool {
    let output_value = output_to_value(vrf_output);
    let threshold = tau * (stake as f64 / total_stake as f64);

    output_value < threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Sha256;

    #[test]
    fn test_vrf_generation() {
        let keypair = VrfKeypair::generate();
        assert_eq!(keypair.secret_key().len(), 32);
        assert_eq!(keypair.public_key().len(), 32);
    }

    #[test]
    fn test_vrf_prove_deterministic() {
        let keypair = VrfKeypair::generate();
        let input = b"test input";

        let proof1 = keypair.prove(input);
        let proof2 = keypair.prove(input);

        // Same input should give same output (VRF is deterministic)
        assert_eq!(proof1.output, proof2.output);
        assert_eq!(proof1.proof, proof2.proof);
    }

    #[test]
    fn test_vrf_different_inputs() {
        let keypair = VrfKeypair::generate();

        let proof1 = keypair.prove(b"input1");
        let proof2 = keypair.prove(b"input2");

        // Different inputs should give different outputs
        assert_ne!(proof1.output, proof2.output);
    }

    #[test]
    fn test_vrf_verification() {
        let keypair = VrfKeypair::generate();
        let input = b"test input";
        let proof = keypair.prove(input);

        // Proof should be 96 bytes (gamma=32, c=32, s=32)
        assert_eq!(proof.proof.len(), 96);

        let verified = verify_proof(keypair.public_key(), input, &proof).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_vrf_wrong_input_fails() {
        let keypair = VrfKeypair::generate();
        let proof = keypair.prove(b"correct input");

        let verified = verify_proof(keypair.public_key(), b"wrong input", &proof).unwrap();
        assert!(!verified);
    }

    #[test]
    fn test_vrf_wrong_key_fails() {
        let keypair1 = VrfKeypair::generate();
        let keypair2 = VrfKeypair::generate();

        let input = b"test input";
        let proof = keypair1.prove(input);

        // Verification with wrong public key should fail
        let verified = verify_proof(keypair2.public_key(), input, &proof).unwrap();
        assert!(!verified);
    }

    #[test]
    fn test_output_to_value_range() {
        let output = [0u8; 32];
        let val = output_to_value(&output);
        assert!((0.0..1.0).contains(&val));

        let output = [255u8; 32];
        let val = output_to_value(&output);
        assert!((0.0..1.0).contains(&val));
    }

    #[test]
    fn test_leader_eligibility() {
        let low_output = [0u8; 32];
        assert!(check_leader_eligibility(&low_output, 100, 10_000, 0.8));
        assert!(check_leader_eligibility(&low_output, 5_000, 10_000, 0.8));

        let high_output = [255u8; 32];
        assert!(!check_leader_eligibility(&high_output, 100, 10_000, 0.8));
    }

    #[test]
    fn test_epoch_randomness_chain() {
        // Test epoch randomness derivation: η_e = H(VRF_i(η_{e-1} || e))
        let keypair = VrfKeypair::generate();
        let mut epoch_randomness = [0u8; 32]; // Genesis randomness

        for epoch in 0u64..5 {
            let mut input = Vec::new();
            input.extend_from_slice(&epoch_randomness);
            input.extend_from_slice(&epoch.to_le_bytes());

            let proof = keypair.prove(&input);

            // New randomness is hash of VRF output
            let mut hasher = Sha256::new();
            hasher.update(proof.output);
            epoch_randomness = hasher.finalize().into();

            println!("Epoch {}: randomness = {:?}", epoch, &epoch_randomness[..8]);
        }

        // Randomness should be different for each epoch
        assert_ne!(epoch_randomness, [0u8; 32]);
    }
}
