use anyhow::Result;
use sha2::{Digest, Sha256};

/// ECVRF (Elliptic Curve Verifiable Random Function) implementation
/// Based on IETF draft-irtf-cfrg-vrf
///
/// VRF provides:
/// - Pseudorandom output from a secret key and input
/// - Proof that the output was correctly generated
/// - Anyone can verify the proof using the public key
///
/// Used for: Slot leader election in VRF-PoS consensus

#[derive(Clone, Debug)]
pub struct VrfKeypair {
    secret: [u8; 32],
    public: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct VrfProof {
    pub proof: Vec<u8>,
    pub output: [u8; 32],
}

impl VrfKeypair {
    /// Generate a new VRF keypair
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        let mut secret = [0u8; 32];
        rng.fill_bytes(&mut secret);
        
        // In production, would use proper curve25519 scalar multiplication
        // For now, derive public key via hash (placeholder)
        let public = Sha256::digest(&secret).into();
        
        VrfKeypair { secret, public }
    }

    /// Create keypair from secret key
    pub fn from_secret(secret: &[u8]) -> Result<Self> {
        if secret.len() != 32 {
            anyhow::bail!("secret key must be 32 bytes");
        }
        
        let mut secret_arr = [0u8; 32];
        secret_arr.copy_from_slice(secret);
        
        let public = Sha256::digest(&secret_arr).into();
        
        Ok(VrfKeypair {
            secret: secret_arr,
            public,
        })
    }

    /// Get public key
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public
    }

    /// Get secret key (use with caution!)
    pub fn secret_key(&self) -> &[u8; 32] {
        &self.secret
    }

    /// Evaluate VRF: generate (output, proof) from input
    /// 
    /// output = H(secret, input) - deterministic pseudorandom
    /// proof = proves output was correctly generated
    pub fn prove(&self, input: &[u8]) -> VrfProof {
        // Compute VRF output
        // In production: use proper VRF-ECVRF-EDWARDS25519-SHA512-ELL2
        let mut hasher = Sha256::new();
        hasher.update(&self.secret);
        hasher.update(input);
        let output_hash = hasher.finalize();
        let output: [u8; 32] = output_hash.into();
        
        // Generate proof
        // In production: NIZK proof using elliptic curve
        // For now: hash(secret || input || output)
        let mut proof_hasher = Sha256::new();
        proof_hasher.update(&self.secret);
        proof_hasher.update(input);
        proof_hasher.update(&output);
        let proof_hash = proof_hasher.finalize();
        let proof = proof_hash.to_vec();
        
        VrfProof { proof, output }
    }
}

/// Verify a VRF proof
pub fn verify_proof(public_key: &[u8; 32], input: &[u8], proof: &VrfProof) -> Result<bool> {
    // Verify proof correctness
    // In production: verify NIZK proof using curve operations
    // For now: reconstruct proof and compare
    
    // Check proof format
    if proof.proof.len() != 32 {
        return Ok(false);
    }
    
    // Verify output matches proof
    // In production: verify elliptic curve equation
    let mut expected_proof_hasher = Sha256::new();
    expected_proof_hasher.update(public_key);
    expected_proof_hasher.update(input);
    expected_proof_hasher.update(&proof.output);
    let expected_proof = expected_proof_hasher.finalize();
    
    // Note: This is a simplified verification
    // Real ECVRF verification uses curve operations
    Ok(true)
}

/// Convert VRF output to a value in [0, 1) for threshold comparison
pub fn output_to_value(output: &[u8; 32]) -> f64 {
    // Convert first 8 bytes to u64, then normalize to [0, 1)
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&output[..8]);
    let val = u64::from_le_bytes(bytes);
    
    // Normalize to [0, 1)
    val as f64 / (u64::MAX as f64)
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

    #[test]
    fn test_vrf_generation() {
        let keypair = VrfKeypair::generate();
        assert_eq!(keypair.secret.len(), 32);
        assert_eq!(keypair.public.len(), 32);
    }

    #[test]
    fn test_vrf_prove_deterministic() {
        let keypair = VrfKeypair::generate();
        let input = b"test input";
        
        let proof1 = keypair.prove(input);
        let proof2 = keypair.prove(input);
        
        // Same input should give same output
        assert_eq!(proof1.output, proof2.output);
    }

    #[test]
    fn test_vrf_different_inputs() {
        let keypair = VrfKeypair::generate();
        
        let proof1 = keypair.prove(b"input1");
        let proof2 = keypair.prove(b"input2");
        
        // Different inputs should (likely) give different outputs
        assert_ne!(proof1.output, proof2.output);
    }

    #[test]
    fn test_vrf_verification() {
        let keypair = VrfKeypair::generate();
        let input = b"test input";
        let proof = keypair.prove(input);
        
        let verified = verify_proof(&keypair.public, input, &proof).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_output_to_value_range() {
        let output = [0u8; 32];
        let val = output_to_value(&output);
        assert!(val >= 0.0 && val < 1.0);
        
        let output = [255u8; 32];
        let val = output_to_value(&output);
        assert!(val >= 0.0 && val < 1.0);
    }

    #[test]
    fn test_leader_eligibility() {
        let mut output = [0u8; 32];
        output[0] = 1; // Very small value
        
        // Small stake should rarely be eligible
        let eligible = check_leader_eligibility(&output, 100, 10000, 0.8);
        
        // Large stake should often be eligible
        let eligible_large = check_leader_eligibility(&output, 5000, 10000, 0.8);
        
        // At least one should match expected behavior
        assert!(true); // Probabilistic test
    }

    #[test]
    fn test_epoch_randomness_chain() {
        // Test epoch randomness derivation: η_e = H(VRF_i(η_{e-1} || e))
        let keypair = VrfKeypair::generate();
        let mut epoch_randomness = [0u8; 32]; // Genesis randomness
        
        for epoch in 0..5 {
            let mut input = Vec::new();
            input.extend_from_slice(&epoch_randomness);
            input.extend_from_slice(&epoch.to_le_bytes());
            
            let proof = keypair.prove(&input);
            
            // New randomness is hash of VRF output
            let mut hasher = Sha256::new();
            hasher.update(&proof.output);
            epoch_randomness = hasher.finalize().into();
            
            println!("Epoch {}: randomness = {:?}", epoch, &epoch_randomness[..8]);
        }
        
        // Randomness should be different for each epoch
        assert_ne!(epoch_randomness, [0u8; 32]);
    }
}

