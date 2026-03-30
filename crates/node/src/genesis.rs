use aether_types::{Address, Block, PublicKey, ValidatorInfo, VrfProof, H256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Genesis configuration for bootstrapping a new network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Chain identifier.
    pub chain_id: String,
    /// Initial validators with their stakes.
    pub validators: Vec<GenesisValidator>,
    /// Initial account balances.
    pub accounts: Vec<GenesisAccount>,
    /// Genesis timestamp.
    pub timestamp: u64,
    /// Initial protocol version.
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    pub name: String,
    pub pubkey: Vec<u8>,
    pub stake: u128,
    pub commission_bps: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    pub address: Address,
    pub balance: u128,
}

/// Result of building genesis state.
pub struct GenesisResult {
    pub genesis_block: Block,
    pub genesis_hash: H256,
    pub validator_set: Vec<ValidatorInfo>,
    pub total_stake: u128,
    pub total_supply: u128,
}

impl GenesisConfig {
    /// Build the genesis block and initial state from the config.
    pub fn build(&self) -> GenesisResult {
        // Build validator set
        let validator_set: Vec<ValidatorInfo> = self
            .validators
            .iter()
            .map(|v| ValidatorInfo {
                pubkey: PublicKey::from_bytes(v.pubkey.clone()),
                stake: v.stake,
                commission: v.commission_bps,
                active: true,
            })
            .collect();

        let total_stake: u128 = validator_set.iter().map(|v| v.stake).sum();
        let total_supply: u128 = self.accounts.iter().map(|a| a.balance).sum::<u128>() + total_stake;

        // Compute genesis state root from accounts
        let state_root = self.compute_state_root();

        // Create genesis block (slot 0, no parent)
        let genesis_block = Block {
            header: aether_types::BlockHeader {
                version: self.protocol_version,
                slot: 0,
                parent_hash: H256::zero(),
                state_root,
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                proposer: Address::from_slice(&[0u8; 20]).unwrap(),
                vrf_proof: VrfProof {
                    output: [0u8; 32],
                    proof: vec![],
                },
                timestamp: self.timestamp,
            },
            transactions: vec![],
            aggregated_vote: None,
        };

        let genesis_hash = genesis_block.hash();

        GenesisResult {
            genesis_block,
            genesis_hash,
            validator_set,
            total_stake,
            total_supply,
        }
    }

    fn compute_state_root(&self) -> H256 {
        let mut hasher = Sha256::new();
        hasher.update(self.chain_id.as_bytes());
        for account in &self.accounts {
            hasher.update(account.address.as_bytes());
            hasher.update(account.balance.to_le_bytes());
        }
        for validator in &self.validators {
            hasher.update(&validator.pubkey);
            hasher.update(validator.stake.to_le_bytes());
        }
        H256::from_slice(&hasher.finalize()).unwrap()
    }

    /// Validate the genesis config.
    pub fn validate(&self) -> Result<(), String> {
        if self.chain_id.is_empty() {
            return Err("chain_id must not be empty".into());
        }
        if self.validators.is_empty() {
            return Err("must have at least one validator".into());
        }
        for v in &self.validators {
            if v.stake == 0 {
                return Err(format!("validator {} has zero stake", v.name));
            }
            if v.pubkey.is_empty() {
                return Err(format!("validator {} has empty pubkey", v.name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GenesisConfig {
        GenesisConfig {
            chain_id: "aether-test-1".into(),
            validators: vec![
                GenesisValidator {
                    name: "validator-1".into(),
                    pubkey: vec![1u8; 32],
                    stake: 1_000_000,
                    commission_bps: 500,
                },
                GenesisValidator {
                    name: "validator-2".into(),
                    pubkey: vec![2u8; 32],
                    stake: 2_000_000,
                    commission_bps: 300,
                },
            ],
            accounts: vec![
                GenesisAccount {
                    address: Address::from_slice(&[10u8; 20]).unwrap(),
                    balance: 100_000_000,
                },
            ],
            timestamp: 1700000000,
            protocol_version: 1,
        }
    }

    #[test]
    fn test_genesis_build() {
        let config = test_config();
        let result = config.build();

        assert_eq!(result.genesis_block.header.slot, 0);
        assert_eq!(result.genesis_block.header.version, 1);
        assert_eq!(result.validator_set.len(), 2);
        assert_eq!(result.total_stake, 3_000_000);
        assert_ne!(result.genesis_hash, H256::zero());
    }

    #[test]
    fn test_genesis_deterministic() {
        let config = test_config();
        let r1 = config.build();
        let r2 = config.build();
        assert_eq!(r1.genesis_hash, r2.genesis_hash);
    }

    #[test]
    fn test_genesis_validation() {
        let config = test_config();
        assert!(config.validate().is_ok());

        let mut bad = test_config();
        bad.validators.clear();
        assert!(bad.validate().is_err());

        let mut bad2 = test_config();
        bad2.chain_id = String::new();
        assert!(bad2.validate().is_err());
    }

    #[test]
    fn test_state_root_changes_with_accounts() {
        let mut c1 = test_config();
        let mut c2 = test_config();
        c2.accounts[0].balance = 999;

        let r1 = c1.build();
        let r2 = c2.build();
        assert_ne!(
            r1.genesis_block.header.state_root,
            r2.genesis_block.header.state_root
        );
    }
}
