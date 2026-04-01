//! Account Abstraction (ERC-4337 style) for Aether.
//!
//! Enables programmable accounts that support:
//! - Custom signature validation (multisig, social recovery)
//! - Gas sponsorship (paymasters pay gas on behalf of users)
//! - Batch operations (multiple calls in one UserOperation)
//!
//! # Architecture
//! - **UserOperation**: A pseudo-transaction describing what the user wants to do
//! - **Bundler**: Aggregates UserOperations and submits them as a single transaction
//! - **EntryPoint**: System program that validates and executes UserOperations
//! - **Paymaster**: Optional gas sponsor

use aether_types::{Address, H256};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// A user operation (ERC-4337 style pseudo-transaction).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserOperation {
    /// The account initiating this operation.
    pub sender: Address,
    /// Anti-replay nonce (managed by the account, not the protocol).
    pub nonce: u64,
    /// Calldata to execute on the sender's account.
    pub call_data: Vec<u8>,
    /// Gas limit for validation + execution.
    pub call_gas_limit: u64,
    /// Gas limit for verification step.
    pub verification_gas_limit: u64,
    /// Gas for pre-verification (bundler overhead).
    pub pre_verification_gas: u64,
    /// Maximum fee per gas unit.
    pub max_fee_per_gas: u128,
    /// Optional: paymaster address that sponsors gas.
    pub paymaster: Option<Address>,
    /// Optional: paymaster-specific data (approval signature, etc.).
    pub paymaster_data: Vec<u8>,
    /// Signature (validated by the account's custom logic, not Ed25519).
    pub signature: Vec<u8>,
}

impl UserOperation {
    /// Hash the UserOperation for signing.
    pub fn hash(&self) -> H256 {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        H256::from_slice(&Sha256::digest(&bytes)).unwrap()
    }

    /// Total gas this operation requires.
    /// Returns `None` if the sum of gas fields overflows u64.
    pub fn total_gas(&self) -> Option<u64> {
        self.call_gas_limit
            .checked_add(self.verification_gas_limit)?
            .checked_add(self.pre_verification_gas)
    }

    /// Validate basic structural constraints.
    pub fn validate(&self) -> Result<()> {
        if self.call_gas_limit == 0 {
            bail!("call_gas_limit must be > 0");
        }
        if self.verification_gas_limit == 0 {
            bail!("verification_gas_limit must be > 0");
        }
        if self.max_fee_per_gas == 0 {
            bail!("max_fee_per_gas must be > 0");
        }
        if self.signature.is_empty() {
            bail!("signature must not be empty");
        }
        Ok(())
    }
}

/// EntryPoint manages UserOperation execution.
pub struct EntryPoint {
    /// Registered smart accounts: address → validation code hash.
    accounts: std::collections::HashMap<Address, H256>,
    /// Registered paymasters: address → deposit balance.
    paymasters: std::collections::HashMap<Address, u128>,
    /// Nonce tracking per account to prevent replay attacks.
    nonces: std::collections::HashMap<Address, u64>,
}

impl EntryPoint {
    pub fn new() -> Self {
        EntryPoint {
            accounts: std::collections::HashMap::new(),
            paymasters: std::collections::HashMap::new(),
            nonces: std::collections::HashMap::new(),
        }
    }

    /// Register a smart account.
    pub fn register_account(&mut self, address: Address, code_hash: H256) {
        self.accounts.insert(address, code_hash);
    }

    /// Register a paymaster with an initial deposit.
    pub fn register_paymaster(&mut self, address: Address, deposit: u128) {
        self.paymasters.insert(address, deposit);
    }

    /// Validate a UserOperation before execution.
    pub fn validate_user_op(&self, op: &UserOperation) -> Result<()> {
        op.validate()?;

        // Check sender is a registered smart account
        if !self.accounts.contains_key(&op.sender) {
            bail!("sender {:?} is not a registered smart account", op.sender);
        }

        // If paymaster is specified, check it's registered and has funds
        if let Some(paymaster) = &op.paymaster {
            let deposit = self
                .paymasters
                .get(paymaster)
                .ok_or_else(|| anyhow::anyhow!("paymaster not registered"))?;

            let total_gas = op
                .total_gas()
                .ok_or_else(|| anyhow::anyhow!("gas field overflow in UserOperation"))?;
            let required_gas_cost = (total_gas as u128)
                .checked_mul(op.max_fee_per_gas)
                .ok_or_else(|| anyhow::anyhow!("gas cost overflow: total_gas * max_fee_per_gas"))?;
            if *deposit < required_gas_cost {
                bail!(
                    "paymaster deposit {} insufficient for gas cost {}",
                    deposit,
                    required_gas_cost
                );
            }
        }

        Ok(())
    }

    /// Execute a batch of UserOperations (bundler submission).
    pub fn handle_ops(&mut self, ops: &[UserOperation]) -> Result<Vec<UserOpResult>> {
        let mut results = Vec::new();

        for op in ops {
            let result = match self.validate_user_op(op) {
                Ok(()) => {
                    // Validate and increment nonce to prevent replay
                    let expected_nonce = self.nonces.get(&op.sender).copied().unwrap_or(0);
                    if op.nonce != expected_nonce {
                        results.push(UserOpResult {
                            op_hash: op.hash(),
                            success: false,
                            gas_used: op.verification_gas_limit,
                            error: Some(format!(
                                "invalid nonce: expected {}, got {}",
                                expected_nonce, op.nonce
                            )),
                        });
                        continue;
                    }
                    self.nonces.insert(op.sender, expected_nonce + 1);

                    // Deduct paymaster deposit if applicable
                    if let Some(paymaster) = &op.paymaster {
                        let total_gas = match op.total_gas() {
                            Some(g) => g,
                            None => {
                                results.push(UserOpResult {
                                    op_hash: op.hash(),
                                    success: false,
                                    gas_used: 0,
                                    error: Some("gas field overflow in UserOperation".to_string()),
                                });
                                continue;
                            }
                        };
                        let cost = match (total_gas as u128).checked_mul(op.max_fee_per_gas) {
                            Some(c) => c,
                            None => {
                                results.push(UserOpResult {
                                    op_hash: op.hash(),
                                    success: false,
                                    gas_used: 0,
                                    error: Some("gas cost overflow".to_string()),
                                });
                                continue;
                            }
                        };
                        if let Some(deposit) = self.paymasters.get_mut(paymaster) {
                            if *deposit < cost {
                                return Err(anyhow::anyhow!(
                                    "insufficient paymaster deposit: have {}, need {}",
                                    deposit,
                                    cost
                                ));
                            }
                            *deposit = deposit
                                .checked_sub(cost)
                                .ok_or(anyhow::anyhow!("deposit underflow"))?;
                        }
                    }

                    UserOpResult {
                        op_hash: op.hash(),
                        success: true,
                        gas_used: op.call_gas_limit, // Simplified
                        error: None,
                    }
                }
                Err(e) => UserOpResult {
                    op_hash: op.hash(),
                    success: false,
                    gas_used: op.verification_gas_limit,
                    error: Some(e.to_string()),
                },
            };
            results.push(result);
        }

        Ok(results)
    }
}

impl Default for EntryPoint {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of executing a UserOperation.
#[derive(Debug, Clone)]
pub struct UserOpResult {
    pub op_hash: H256,
    pub success: bool,
    pub gas_used: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user_op(sender_byte: u8) -> UserOperation {
        UserOperation {
            sender: Address::from_slice(&[sender_byte; 20]).unwrap(),
            nonce: 0,
            call_data: vec![1, 2, 3],
            call_gas_limit: 100_000,
            verification_gas_limit: 50_000,
            pre_verification_gas: 10_000,
            max_fee_per_gas: 100,
            paymaster: None,
            paymaster_data: vec![],
            signature: vec![0u8; 64],
        }
    }

    #[test]
    fn test_user_op_validation() {
        let op = make_user_op(1);
        assert!(op.validate().is_ok());
    }

    #[test]
    fn test_reject_empty_signature() {
        let mut op = make_user_op(1);
        op.signature = vec![];
        assert!(op.validate().is_err());
    }

    #[test]
    fn test_user_op_hash_deterministic() {
        let op = make_user_op(1);
        assert_eq!(op.hash(), op.hash());
    }

    #[test]
    fn test_entrypoint_validates_registered_account() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        ep.register_account(sender, H256::from_slice(&[2u8; 32]).unwrap());

        let op = make_user_op(1);
        assert!(ep.validate_user_op(&op).is_ok());
    }

    #[test]
    fn test_entrypoint_rejects_unregistered_account() {
        let ep = EntryPoint::new();
        let op = make_user_op(1);
        assert!(ep.validate_user_op(&op).is_err());
    }

    #[test]
    fn test_paymaster_sponsorship() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        let paymaster = Address::from_slice(&[2u8; 20]).unwrap();

        ep.register_account(sender, H256::zero());
        ep.register_paymaster(paymaster, 100_000_000); // Large deposit

        let mut op = make_user_op(1);
        op.paymaster = Some(paymaster);

        assert!(ep.validate_user_op(&op).is_ok());
    }

    #[test]
    fn test_paymaster_insufficient_deposit() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        let paymaster = Address::from_slice(&[2u8; 20]).unwrap();

        ep.register_account(sender, H256::zero());
        ep.register_paymaster(paymaster, 1); // Tiny deposit

        let mut op = make_user_op(1);
        op.paymaster = Some(paymaster);

        assert!(ep.validate_user_op(&op).is_err());
    }

    #[test]
    fn test_handle_ops_batch() {
        let mut ep = EntryPoint::new();
        let s1 = Address::from_slice(&[1u8; 20]).unwrap();
        let s2 = Address::from_slice(&[2u8; 20]).unwrap();

        ep.register_account(s1, H256::zero());
        ep.register_account(s2, H256::zero());

        let ops = vec![make_user_op(1), make_user_op(2)];
        let results = ep.handle_ops(&ops).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);
    }

    #[test]
    fn test_total_gas() {
        let op = make_user_op(1);
        assert_eq!(op.total_gas(), Some(100_000 + 50_000 + 10_000));

        // Overflow case: all fields at u64::MAX / 3 + 1 overflows
        let mut overflow_op = make_user_op(99);
        overflow_op.call_gas_limit = u64::MAX / 2;
        overflow_op.verification_gas_limit = u64::MAX / 2;
        overflow_op.pre_verification_gas = 2;
        assert!(overflow_op.total_gas().is_none(), "overflow must return None");
    }
}
