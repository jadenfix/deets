// ============================================================================
// HYBRID NODE - Phase 1 Full Integration Helper
// ============================================================================
// Wires VRF+HotStuff+BLS consensus into the node
// ============================================================================

use aether_consensus::HybridConsensus;
use aether_crypto_bls::BlsKeypair;
use aether_crypto_primitives::Keypair;
use aether_crypto_vrf::VrfKeypair;
use aether_types::{Address, PublicKey, ValidatorInfo};
use anyhow::Result;

/// Keypair bundle for a validator in hybrid consensus
pub struct ValidatorKeypair {
    pub ed25519: Keypair,
    pub vrf: VrfKeypair,
    pub bls: BlsKeypair,
}

impl ValidatorKeypair {
    /// Generate a new set of validator keys
    pub fn generate() -> Self {
        Self {
            ed25519: Keypair::generate(),
            vrf: VrfKeypair::generate(),
            bls: BlsKeypair::generate(),
        }
    }

    /// Get the validator's address
    pub fn address(&self) -> Address {
        let pubkey = PublicKey::from_bytes(self.ed25519.public_key());
        pubkey.to_address()
    }

    /// Get the validator's public key
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.ed25519.public_key())
    }
}

/// Create a hybrid consensus engine with VRF + HotStuff + BLS
pub fn create_hybrid_consensus(
    validators: Vec<ValidatorInfo>,
    my_keypair: Option<&ValidatorKeypair>,
    tau: f64,
    epoch_length: u64,
) -> Result<HybridConsensus> {
    let (my_vrf, my_bls, my_addr) = if let Some(kp) = my_keypair {
        (
            Some(kp.vrf.clone()),
            Some(kp.bls.clone()),
            Some(kp.address()),
        )
    } else {
        (None, None, None)
    };

    let mut consensus = HybridConsensus::new(
        validators,
        tau,
        epoch_length,
        my_vrf,
        my_bls,
        my_addr,
    );

    // Register the local validator's BLS key so its own votes are accepted
    if let Some(kp) = my_keypair {
        let addr = kp.address();
        let bls_pk = kp.bls.public_key();
        let pop = kp.bls.proof_of_possession();
        if let Err(e) = consensus.register_bls_pubkey(addr, bls_pk, &pop) {
            eprintln!("WARNING: failed to register local BLS key: {e}");
        }
    }

    Ok(consensus)
}

/// Create a hybrid consensus engine with VRF and BLS public keys for multi-validator verification.
pub fn create_hybrid_consensus_with_vrf_keys(
    validators: Vec<ValidatorInfo>,
    vrf_pubkeys: Vec<(Address, [u8; 32])>,
    my_keypair: Option<&ValidatorKeypair>,
    tau: f64,
    epoch_length: u64,
) -> Result<HybridConsensus> {
    let (my_vrf, my_bls, my_addr) = if let Some(kp) = my_keypair {
        (
            Some(kp.vrf.clone()),
            Some(kp.bls.clone()),
            Some(kp.address()),
        )
    } else {
        (None, None, None)
    };

    let mut consensus = HybridConsensus::new(
        validators,
        tau,
        epoch_length,
        my_vrf,
        my_bls,
        my_addr,
    );

    // Register all VRF public keys for cross-validation
    for (addr, vrf_pk) in vrf_pubkeys {
        consensus.register_vrf_pubkey(addr, vrf_pk);
    }

    // Register the local validator's BLS key so votes are accepted
    if let Some(kp) = my_keypair {
        let addr = kp.address();
        let bls_pk = kp.bls.public_key();
        let pop = kp.bls.proof_of_possession();
        if let Err(e) = consensus.register_bls_pubkey(addr, bls_pk, &pop) {
            eprintln!("WARNING: failed to register local BLS key: {e}");
        }
    }

    Ok(consensus)
}

/// Create a hybrid consensus with ALL validators' BLS keys registered.
/// Required for multi-validator E2E scenarios where each node needs to verify
/// votes from every other validator.
pub fn create_hybrid_consensus_with_all_keys(
    validators: Vec<ValidatorInfo>,
    vrf_pubkeys: Vec<(Address, [u8; 32])>,
    bls_pubkeys: Vec<(Address, Vec<u8>, Vec<u8>)>, // (addr, pubkey, pop_signature)
    my_keypair: Option<&ValidatorKeypair>,
    tau: f64,
    epoch_length: u64,
) -> Result<HybridConsensus> {
    let mut consensus = create_hybrid_consensus_with_vrf_keys(
        validators,
        vrf_pubkeys,
        my_keypair,
        tau,
        epoch_length,
    )?;

    // Register ALL validators' BLS keys for cross-validation
    for (addr, bls_pk, pop) in bls_pubkeys {
        if let Err(e) = consensus.register_bls_pubkey(addr, bls_pk, &pop) {
            eprintln!("WARNING: failed to register BLS key for {:?}: {e}", addr);
        }
    }

    Ok(consensus)
}

/// Helper to create validator info from a keypair
pub fn validator_info_from_keypair(keypair: &ValidatorKeypair, stake: u128) -> ValidatorInfo {
    ValidatorInfo {
        pubkey: keypair.public_key(),
        stake,
        commission: 1000, // 10%
        active: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_consensus::ConsensusEngine;

    #[test]
    fn test_validator_keypair_generation() {
        let keypair = ValidatorKeypair::generate();
        let address = keypair.address();
        let pubkey = keypair.public_key();

        assert_eq!(address, pubkey.to_address());
    }

    #[test]
    fn test_hybrid_consensus_creation() {
        let keypair = ValidatorKeypair::generate();
        let validators = vec![validator_info_from_keypair(&keypair, 1_000_000)];

        let consensus = create_hybrid_consensus(validators, Some(&keypair), 0.8, 100).unwrap();

        assert_eq!(consensus.total_stake(), 1_000_000);
    }
}
