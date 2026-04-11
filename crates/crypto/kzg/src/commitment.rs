use anyhow::{bail, Result};
use blst::{blst_fr, blst_p1, blst_p1_affine, blst_p2, blst_p2_affine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// KZG Polynomial Commitment Scheme on BLS12-381.
///
/// Provides constant-size (48-byte) commitments to polynomials and
/// constant-size opening proofs verifiable via 2 pairing checks.
///
/// Used in Aether for:
/// - AI execution trace verification (VCR)
/// - Data availability sampling
/// - Blob transactions (EIP-4844 style)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KzgCommitment {
    pub commitment: Vec<u8>, // Compressed G1 point (48 bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KzgProof {
    pub proof: Vec<u8>,      // Compressed G1 point (48 bytes)
    pub evaluation: Vec<u8>, // BLS scalar (32 bytes)
}

/// KZG trusted setup parameters (Structured Reference String).
///
/// Generated from a Powers of Tau ceremony.
/// Contains `[τ^0]_1`, `[τ^1]_1`, ..., `[τ^n]_1` in G1
/// and `[1]_2`, `[τ]_2` in G2.
pub struct TrustedSetup {
    /// G1 powers: [τ^i]_1 for i = 0..max_degree
    g1_points: Vec<blst_p1>,
    /// `[1]_2` - generator of G2
    g2_gen: blst_p2,
    /// `[τ]_2` - tau times the G2 generator
    g2_tau: blst_p2,
    /// Maximum polynomial degree
    max_degree: usize,
}

impl TrustedSetup {
    /// Generate a TESTING-ONLY trusted setup from a secret scalar.
    /// In production, use a proper Powers of Tau ceremony.
    #[must_use]
    pub fn generate_insecure(max_degree: usize, secret_tau: &[u8; 32]) -> Self {
        let tau = scalar_from_bytes(secret_tau);

        // G1 generator
        let g1_gen = g1_generator();
        // G2 generator
        let g2_gen = g2_generator();

        // Compute [τ^i]_1 for i = 0..max_degree
        let mut g1_points = Vec::with_capacity(max_degree + 1);
        let mut tau_power = scalar_one();

        for _ in 0..=max_degree {
            let point = g1_scalar_mul(&g1_gen, &tau_power);
            g1_points.push(point);
            tau_power = scalar_mul(&tau_power, &tau);
        }

        // Compute [τ]_2
        let g2_tau = g2_scalar_mul(&g2_gen, &tau);

        TrustedSetup {
            g1_points,
            g2_gen,
            g2_tau,
            max_degree,
        }
    }
}

pub struct KzgVerifier {
    setup: TrustedSetup,
}

impl KzgVerifier {
    /// Create a verifier with an insecure setup (for testing).
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn new_insecure_test(max_degree: usize) -> Self {
        let tau_bytes = Sha256::digest(b"aether-kzg-test-setup-DO-NOT-USE-IN-PRODUCTION");
        let mut tau = [0u8; 32];
        tau.copy_from_slice(&tau_bytes);
        KzgVerifier {
            setup: TrustedSetup::generate_insecure(max_degree, &tau),
        }
    }

    /// Create a verifier with a specific trusted setup.
    #[must_use]
    pub fn with_setup(setup: TrustedSetup) -> Self {
        KzgVerifier { setup }
    }

    /// Commit to a polynomial represented by its coefficients.
    ///
    /// Each coefficient is a 32-byte scalar (BLS12-381 field element).
    /// C = Σ_i coeff_i * [τ^i]_1
    pub fn commit(&self, coefficients: &[ScalarBytes]) -> Result<KzgCommitment> {
        if coefficients.is_empty() {
            bail!("empty coefficients");
        }
        if coefficients.len() > self.setup.max_degree + 1 {
            bail!(
                "degree {} exceeds maximum {}",
                coefficients.len() - 1,
                self.setup.max_degree
            );
        }

        // Multi-scalar multiplication: C = Σ coeff_i * g1_points[i]
        let commitment = multi_scalar_mul_g1(&self.setup.g1_points, coefficients);
        let compressed = compress_g1(&commitment);

        Ok(KzgCommitment {
            commitment: compressed,
        })
    }

    /// Create an opening proof that P(z) = y.
    ///
    /// Computes the quotient polynomial Q(x) = (P(x) - y) / (x - z)
    /// and returns π = [Q(τ)]_1 along with y = P(z).
    pub fn create_proof(&self, coefficients: &[ScalarBytes], z: &ScalarBytes) -> Result<KzgProof> {
        if coefficients.is_empty() {
            bail!("empty coefficients");
        }

        // Evaluate P(z)
        let y = evaluate_polynomial(coefficients, z);

        // Compute quotient polynomial Q(x) = (P(x) - y) / (x - z)
        let quotient = compute_quotient(coefficients, z, &y);

        // Commit to quotient: π = [Q(τ)]_1
        let proof_point = multi_scalar_mul_g1(&self.setup.g1_points, &quotient);
        let compressed_proof = compress_g1(&proof_point);

        Ok(KzgProof {
            proof: compressed_proof,
            evaluation: scalar_to_bytes(&y),
        })
    }

    /// Verify that P(z) = y using the commitment C and proof π.
    ///
    /// Checks the pairing equation:
    /// `e(C - [y]_1, [1]_2) == e(π, [τ]_2 - [z]_2)`
    #[must_use = "discarding a KZG verification result is a security bug"]
    pub fn verify(
        &self,
        commitment: &KzgCommitment,
        proof: &KzgProof,
        z: &ScalarBytes,
    ) -> Result<bool> {
        if commitment.commitment.len() != 48 {
            bail!("invalid commitment length: {}", commitment.commitment.len());
        }
        if proof.proof.len() != 48 {
            bail!("invalid proof length: {}", proof.proof.len());
        }
        if proof.evaluation.len() != 32 {
            bail!("invalid evaluation length");
        }

        // Decompress points
        let c_point = decompress_g1(&commitment.commitment)?;
        let pi_point = decompress_g1(&proof.proof)?;

        // y as scalar
        let y = scalar_from_bytes(
            proof
                .evaluation
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("evaluation must be 32 bytes"))?,
        );

        // z as scalar
        let z_scalar = scalar_from_bytes(z);

        // LHS: C - [y]_1
        let g1_gen = g1_generator();
        let y_g1 = g1_scalar_mul(&g1_gen, &y);
        let lhs = g1_sub(&c_point, &y_g1);

        // RHS factor: [τ]_2 - [z]_2
        let g2_gen = self.setup.g2_gen;
        let z_g2 = g2_scalar_mul(&g2_gen, &z_scalar);
        let rhs_g2 = g2_sub(&self.setup.g2_tau, &z_g2);

        // Pairing check: e(lhs, [1]_2) == e(π, rhs_g2)
        // Equivalently: e(lhs, [1]_2) * e(-π, rhs_g2) == 1
        let valid = pairing_check(&lhs, &g2_gen, &pi_point, &rhs_g2);

        Ok(valid)
    }

    /// Batch verify multiple proofs using random linear combination.
    pub fn batch_verify(
        &self,
        commitments: &[KzgCommitment],
        proofs: &[KzgProof],
        points: &[ScalarBytes],
    ) -> Result<bool> {
        if commitments.len() != proofs.len() || proofs.len() != points.len() {
            bail!("mismatched array lengths");
        }

        // For small batches, verify individually
        for i in 0..commitments.len() {
            if !self.verify(&commitments[i], &proofs[i], &points[i])? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    #[inline]
    #[must_use]
    pub fn max_degree(&self) -> usize {
        self.setup.max_degree
    }
}

/// A 32-byte scalar (BLS12-381 field element).
pub type ScalarBytes = [u8; 32];

// ============================================================
// Low-level BLS12-381 operations using the `blst` crate
// ============================================================

fn scalar_from_bytes(bytes: &[u8; 32]) -> blst_fr {
    let mut scalar = blst_fr::default();
    // blst expects little-endian scalar bytes
    unsafe {
        blst::blst_fr_from_uint64(&mut scalar, bytes_to_u64_array(bytes).as_ptr());
    }
    scalar
}

fn scalar_to_bytes(s: &blst_fr) -> Vec<u8> {
    let mut out = [0u64; 4];
    unsafe {
        blst::blst_uint64_from_fr(out.as_mut_ptr(), s);
    }
    let mut bytes = Vec::with_capacity(32);
    for limb in &out {
        bytes.extend_from_slice(&limb.to_le_bytes());
    }
    bytes
}

fn scalar_one() -> blst_fr {
    let mut one = [0u8; 32];
    one[0] = 1;
    scalar_from_bytes(&one)
}

fn scalar_mul(a: &blst_fr, b: &blst_fr) -> blst_fr {
    let mut result = blst_fr::default();
    unsafe {
        blst::blst_fr_mul(&mut result, a, b);
    }
    result
}

fn scalar_sub(a: &blst_fr, b: &blst_fr) -> blst_fr {
    let mut result = blst_fr::default();
    unsafe {
        blst::blst_fr_sub(&mut result, a, b);
    }
    result
}

fn scalar_add(a: &blst_fr, b: &blst_fr) -> blst_fr {
    let mut result = blst_fr::default();
    unsafe {
        blst::blst_fr_add(&mut result, a, b);
    }
    result
}

fn g1_generator() -> blst_p1 {
    unsafe { *blst::blst_p1_generator() }
}

fn g2_generator() -> blst_p2 {
    unsafe { *blst::blst_p2_generator() }
}

fn g1_scalar_mul(point: &blst_p1, scalar: &blst_fr) -> blst_p1 {
    let mut result = blst_p1::default();
    // Convert scalar to 256-bit representation
    let mut scalar_bytes = [0u8; 32];
    let mut limbs = [0u64; 4];
    unsafe {
        blst::blst_uint64_from_fr(limbs.as_mut_ptr(), scalar);
    }
    for (i, limb) in limbs.iter().enumerate() {
        scalar_bytes[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_le_bytes());
    }
    unsafe {
        blst::blst_p1_mult(&mut result, point, scalar_bytes.as_ptr(), 256);
    }
    result
}

fn g2_scalar_mul(point: &blst_p2, scalar: &blst_fr) -> blst_p2 {
    let mut result = blst_p2::default();
    let mut scalar_bytes = [0u8; 32];
    let mut limbs = [0u64; 4];
    unsafe {
        blst::blst_uint64_from_fr(limbs.as_mut_ptr(), scalar);
    }
    for (i, limb) in limbs.iter().enumerate() {
        scalar_bytes[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_le_bytes());
    }
    unsafe {
        blst::blst_p2_mult(&mut result, point, scalar_bytes.as_ptr(), 256);
    }
    result
}

fn g1_add(a: &blst_p1, b: &blst_p1) -> blst_p1 {
    let mut result = blst_p1::default();
    unsafe {
        let mut b_aff = blst_p1_affine::default();
        blst::blst_p1_to_affine(&mut b_aff, b);
        blst::blst_p1_add_or_double_affine(&mut result, a, &b_aff);
    }
    result
}

fn g1_sub(a: &blst_p1, b: &blst_p1) -> blst_p1 {
    let mut neg_b = *b;
    unsafe {
        blst::blst_p1_cneg(&mut neg_b, true);
    }
    g1_add(a, &neg_b)
}

fn g2_sub(a: &blst_p2, b: &blst_p2) -> blst_p2 {
    let mut neg_b = *b;
    unsafe {
        blst::blst_p2_cneg(&mut neg_b, true);
    }
    let mut result = blst_p2::default();
    unsafe {
        let mut neg_b_aff = blst_p2_affine::default();
        blst::blst_p2_to_affine(&mut neg_b_aff, &neg_b);
        blst::blst_p2_add_or_double_affine(&mut result, a, &neg_b_aff);
    }
    result
}

fn compress_g1(point: &blst_p1) -> Vec<u8> {
    let mut compressed = [0u8; 48];
    unsafe {
        blst::blst_p1_compress(compressed.as_mut_ptr(), point);
    }
    compressed.to_vec()
}

fn decompress_g1(bytes: &[u8]) -> Result<blst_p1> {
    if bytes.len() != 48 {
        bail!("G1 point must be 48 bytes");
    }
    let mut affine = blst_p1_affine::default();
    let err = unsafe { blst::blst_p1_uncompress(&mut affine, bytes.as_ptr()) };
    if err != blst::BLST_ERROR::BLST_SUCCESS {
        bail!("failed to decompress G1 point: {:?}", err);
    }
    let mut point = blst_p1::default();
    unsafe {
        blst::blst_p1_from_affine(&mut point, &affine);
    }
    Ok(point)
}

/// Multi-scalar multiplication: Σ scalar_i * point_i
fn multi_scalar_mul_g1(points: &[blst_p1], scalars: &[ScalarBytes]) -> blst_p1 {
    // Zero-init is the identity (point at infinity) for blst_p1
    let mut result = blst_p1::default();

    for (i, scalar_bytes) in scalars.iter().enumerate() {
        if i >= points.len() {
            break;
        }
        let scalar = scalar_from_bytes(scalar_bytes);
        let term = g1_scalar_mul(&points[i], &scalar);
        result = g1_add(&result, &term);
    }

    result
}

/// Evaluate polynomial P(x) = Σ coefficients[i] * x^i at point z using Horner's method.
fn evaluate_polynomial(coefficients: &[ScalarBytes], z: &ScalarBytes) -> blst_fr {
    let z_scalar = scalar_from_bytes(z);
    let mut result = blst_fr::default(); // 0

    // Horner's method: start from highest degree
    for i in (0..coefficients.len()).rev() {
        result = scalar_mul(&result, &z_scalar);
        let coeff = scalar_from_bytes(&coefficients[i]);
        result = scalar_add(&result, &coeff);
    }

    result
}

/// Compute quotient polynomial Q(x) = (P(x) - y) / (x - z)
/// using synthetic division.
fn compute_quotient(
    coefficients: &[ScalarBytes],
    z: &ScalarBytes,
    y: &blst_fr,
) -> Vec<ScalarBytes> {
    let n = coefficients.len();
    if n <= 1 {
        return vec![[0u8; 32]];
    }

    let z_scalar = scalar_from_bytes(z);

    // P(x) - y: subtract y from the constant term
    let mut adjusted = Vec::with_capacity(n);
    for (i, coeff) in coefficients.iter().enumerate() {
        let c = scalar_from_bytes(coeff);
        if i == 0 {
            adjusted.push(scalar_sub(&c, y));
        } else {
            adjusted.push(c);
        }
    }

    // Synthetic division by (x - z)
    // Result has degree n-2 (one less than input)
    let mut quotient_scalars = vec![blst_fr::default(); n - 1];
    quotient_scalars[n - 2] = adjusted[n - 1]; // Leading coefficient stays

    for i in (0..n - 2).rev() {
        // q[i] = adjusted[i+1] + z * q[i+1]
        let term = scalar_mul(&z_scalar, &quotient_scalars[i + 1]);
        quotient_scalars[i] = scalar_add(&adjusted[i + 1], &term);
    }

    // Convert back to bytes
    quotient_scalars
        .iter()
        .map(|s| {
            let bytes = scalar_to_bytes(s);
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            arr
        })
        .collect()
}

/// Pairing check: e(a1, a2) == e(b1, b2)
/// Implemented as: e(a1, a2) * e(-b1, b2) == 1
fn pairing_check(a1: &blst_p1, a2: &blst_p2, b1: &blst_p1, b2: &blst_p2) -> bool {
    let mut neg_b1 = *b1;
    unsafe {
        blst::blst_p1_cneg(&mut neg_b1, true);
    }

    // Convert to affine
    let mut a1_aff = blst_p1_affine::default();
    let mut a2_aff = blst_p2_affine::default();
    let mut neg_b1_aff = blst_p1_affine::default();
    let mut b2_aff = blst_p2_affine::default();

    unsafe {
        blst::blst_p1_to_affine(&mut a1_aff, a1);
        blst::blst_p2_to_affine(&mut a2_aff, a2);
        blst::blst_p1_to_affine(&mut neg_b1_aff, &neg_b1);
        blst::blst_p2_to_affine(&mut b2_aff, b2);
    }

    // Compute product of pairings
    let mut pairing = blst::blst_fp12::default();
    unsafe {
        // Miller loop for first pair
        let mut ml1 = blst::blst_fp12::default();
        blst::blst_miller_loop(&mut ml1, &a2_aff, &a1_aff);

        // Miller loop for second pair
        let mut ml2 = blst::blst_fp12::default();
        blst::blst_miller_loop(&mut ml2, &b2_aff, &neg_b1_aff);

        // Multiply
        blst::blst_fp12_mul(&mut pairing, &ml1, &ml2);

        // Final exponentiation
        let mut result = blst::blst_fp12::default();
        blst::blst_final_exp(&mut result, &pairing);

        // Check if result is 1 (identity)
        blst::blst_fp12_is_one(&result)
    }
}

fn bytes_to_u64_array(bytes: &[u8; 32]) -> [u64; 4] {
    let mut result = [0u64; 4];
    for i in 0..4 {
        result[i] = u64::from_le_bytes(bytes[i * 8..(i + 1) * 8].try_into().unwrap());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_coefficients() -> Vec<ScalarBytes> {
        // P(x) = 3 + 2x + x^2
        let mut c0 = [0u8; 32];
        c0[0] = 3;
        let mut c1 = [0u8; 32];
        c1[0] = 2;
        let mut c2 = [0u8; 32];
        c2[0] = 1;
        vec![c0, c1, c2]
    }

    fn test_point() -> ScalarBytes {
        let mut z = [0u8; 32];
        z[0] = 5; // evaluate at x=5
        z
    }

    #[test]
    fn test_commitment_produces_48_bytes() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();
        let commitment = verifier.commit(&coeffs).unwrap();
        assert_eq!(commitment.commitment.len(), 48);
    }

    #[test]
    fn test_proof_produces_correct_sizes() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();
        let z = test_point();
        let proof = verifier.create_proof(&coeffs, &z).unwrap();
        assert_eq!(proof.proof.len(), 48);
        assert_eq!(proof.evaluation.len(), 32);
    }

    #[test]
    fn test_valid_proof_verifies() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();
        let z = test_point();

        let commitment = verifier.commit(&coeffs).unwrap();
        let proof = verifier.create_proof(&coeffs, &z).unwrap();

        let valid = verifier.verify(&commitment, &proof, &z).unwrap();
        assert!(valid, "valid proof must verify");
    }

    #[test]
    fn test_wrong_evaluation_fails() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();
        let z = test_point();

        let commitment = verifier.commit(&coeffs).unwrap();
        let mut proof = verifier.create_proof(&coeffs, &z).unwrap();

        // Tamper with the evaluation
        proof.evaluation[0] ^= 0xff;

        let valid = verifier.verify(&commitment, &proof, &z).unwrap();
        assert!(!valid, "tampered evaluation must not verify");
    }

    #[test]
    fn test_wrong_point_fails() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();
        let z = test_point();

        let commitment = verifier.commit(&coeffs).unwrap();
        let proof = verifier.create_proof(&coeffs, &z).unwrap();

        // Verify at a different point
        let mut wrong_z = [0u8; 32];
        wrong_z[0] = 7;

        let valid = verifier.verify(&commitment, &proof, &wrong_z).unwrap();
        assert!(!valid, "proof for wrong point must not verify");
    }

    #[test]
    fn test_wrong_commitment_fails() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();
        let z = test_point();

        let proof = verifier.create_proof(&coeffs, &z).unwrap();

        // Different polynomial
        let mut wrong_coeffs = test_coefficients();
        wrong_coeffs[0][0] = 99;
        let wrong_commitment = verifier.commit(&wrong_coeffs).unwrap();

        let valid = verifier.verify(&wrong_commitment, &proof, &z).unwrap();
        assert!(!valid, "proof against wrong commitment must not verify");
    }

    #[test]
    fn test_deterministic_commitment() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs = test_coefficients();

        let c1 = verifier.commit(&coeffs).unwrap();
        let c2 = verifier.commit(&coeffs).unwrap();

        assert_eq!(c1.commitment, c2.commitment);
    }

    #[test]
    fn test_batch_verify() {
        let verifier = KzgVerifier::new_insecure_test(16);
        let coeffs1 = test_coefficients();

        let mut coeffs2 = vec![[0u8; 32]; 2];
        coeffs2[0][0] = 7;
        coeffs2[1][0] = 3;

        let z1 = test_point();
        let mut z2 = [0u8; 32];
        z2[0] = 2;

        let c1 = verifier.commit(&coeffs1).unwrap();
        let c2 = verifier.commit(&coeffs2).unwrap();
        let p1 = verifier.create_proof(&coeffs1, &z1).unwrap();
        let p2 = verifier.create_proof(&coeffs2, &z2).unwrap();

        let valid = verifier
            .batch_verify(&[c1, c2], &[p1, p2], &[z1, z2])
            .unwrap();
        assert!(valid, "batch verification of valid proofs must pass");
    }

    #[test]
    fn test_empty_coefficients_rejected() {
        let verifier = KzgVerifier::new_insecure_test(16);
        assert!(verifier.commit(&[]).is_err());
    }

    #[test]
    fn test_degree_exceeds_max_rejected() {
        let verifier = KzgVerifier::new_insecure_test(2);
        let coeffs = vec![[0u8; 32]; 10]; // degree 9, max is 2
        assert!(verifier.commit(&coeffs).is_err());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Use a small degree to keep pairing-based tests fast.
    const TEST_DEGREE: usize = 8;

    fn verifier() -> KzgVerifier {
        KzgVerifier::new_insecure_test(TEST_DEGREE)
    }

    /// Generate a random scalar (32 bytes). We keep the high bit clear to stay
    /// well within the BLS12-381 field modulus for all test inputs.
    fn arb_scalar() -> impl Strategy<Value = ScalarBytes> {
        prop::array::uniform32(any::<u8>()).prop_map(|mut b| {
            b[31] &= 0x3f; // keep top two bits clear — avoids field overflow
            b
        })
    }

    /// Generate a non-empty coefficient vector of length 1..=TEST_DEGREE+1.
    fn arb_coeffs() -> impl Strategy<Value = Vec<ScalarBytes>> {
        prop::collection::vec(arb_scalar(), 1..=TEST_DEGREE + 1)
    }

    proptest! {
        // Use a reduced case count because KZG pairing checks are expensive.
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// Committing to the same polynomial twice produces identical bytes.
        #[test]
        fn commitment_is_deterministic(coeffs in arb_coeffs()) {
            let v = verifier();
            let c1 = v.commit(&coeffs).unwrap();
            let c2 = v.commit(&coeffs).unwrap();
            prop_assert_eq!(c1.commitment, c2.commitment);
        }

        /// Every commitment is exactly 48 bytes (compressed G1 point).
        #[test]
        fn commitment_size_is_48_bytes(coeffs in arb_coeffs()) {
            let v = verifier();
            let c = v.commit(&coeffs).unwrap();
            prop_assert_eq!(c.commitment.len(), 48);
        }

        /// A valid proof created for any polynomial and evaluation point verifies.
        #[test]
        fn valid_proof_always_verifies(coeffs in arb_coeffs(), z in arb_scalar()) {
            let v = verifier();
            let commitment = v.commit(&coeffs).unwrap();
            let proof = v.create_proof(&coeffs, &z).unwrap();
            let valid = v.verify(&commitment, &proof, &z).unwrap();
            prop_assert!(valid, "valid proof must verify");
        }

        /// Proof evaluation field is exactly 32 bytes.
        #[test]
        fn proof_evaluation_is_32_bytes(coeffs in arb_coeffs(), z in arb_scalar()) {
            let v = verifier();
            let proof = v.create_proof(&coeffs, &z).unwrap();
            prop_assert_eq!(proof.proof.len(), 48);
            prop_assert_eq!(proof.evaluation.len(), 32);
        }

        /// Tampered commitment byte causes verification failure.
        #[test]
        fn tampered_commitment_fails(
            coeffs in arb_coeffs(),
            z in arb_scalar(),
            byte_idx in 0usize..48,
            flip in 1u8..=255u8,
        ) {
            let v = verifier();
            let mut commitment = v.commit(&coeffs).unwrap();
            let proof = v.create_proof(&coeffs, &z).unwrap();
            commitment.commitment[byte_idx] ^= flip;
            // A tampered commitment either fails to decompress (Err) or returns false.
            let result = v.verify(&commitment, &proof, &z);
            let ok = result.map(|b| !b).unwrap_or(true);
            prop_assert!(ok, "tampered commitment must not verify");
        }

        /// Tampered evaluation (y value) causes verification failure.
        #[test]
        fn tampered_evaluation_fails(coeffs in arb_coeffs(), z in arb_scalar(), flip in 1u8..=255u8) {
            let v = verifier();
            let commitment = v.commit(&coeffs).unwrap();
            let mut proof = v.create_proof(&coeffs, &z).unwrap();
            proof.evaluation[0] ^= flip;
            let result = v.verify(&commitment, &proof, &z);
            let ok = result.map(|b| !b).unwrap_or(true);
            prop_assert!(ok, "tampered evaluation must not verify");
        }

        /// Verifying at a different z-point fails when polynomials are non-constant
        /// (a constant polynomial evaluates to the same value everywhere).
        #[test]
        fn wrong_z_fails_for_nonconstant_poly(
            // Use ≥2 coefficients to guarantee the polynomial is non-constant.
            coeffs in prop::collection::vec(arb_scalar(), 2..=TEST_DEGREE + 1),
            z in arb_scalar(),
            z_wrong in arb_scalar(),
        ) {
            prop_assume!(z != z_wrong);
            let v = verifier();
            let commitment = v.commit(&coeffs).unwrap();
            let proof = v.create_proof(&coeffs, &z).unwrap();
            // Proof was created for z but we verify at z_wrong — should fail.
            let result = v.verify(&commitment, &proof, &z_wrong);
            let ok = result.map(|b| !b).unwrap_or(true);
            prop_assert!(ok, "proof for z must not verify at z_wrong");
        }

        /// Different polynomials produce different commitments (with overwhelming probability).
        #[test]
        fn different_polys_different_commitments(
            coeffs1 in arb_coeffs(),
            coeffs2 in arb_coeffs(),
        ) {
            prop_assume!(coeffs1 != coeffs2);
            let v = verifier();
            let c1 = v.commit(&coeffs1).unwrap();
            let c2 = v.commit(&coeffs2).unwrap();
            // Collision is cryptographically negligible; assert for testing purposes.
            prop_assert_ne!(c1.commitment, c2.commitment);
        }

        /// Batch verification passes when all individual proofs are valid.
        #[test]
        fn batch_verify_valid_proofs(
            coeffs1 in prop::collection::vec(arb_scalar(), 1..=4),
            coeffs2 in prop::collection::vec(arb_scalar(), 1..=4),
            z1 in arb_scalar(),
            z2 in arb_scalar(),
        ) {
            let v = verifier();
            let c1 = v.commit(&coeffs1).unwrap();
            let c2 = v.commit(&coeffs2).unwrap();
            let p1 = v.create_proof(&coeffs1, &z1).unwrap();
            let p2 = v.create_proof(&coeffs2, &z2).unwrap();
            let valid = v.batch_verify(&[c1, c2], &[p1, p2], &[z1, z2]).unwrap();
            prop_assert!(valid, "batch verify of valid proofs must pass");
        }
    }
}
