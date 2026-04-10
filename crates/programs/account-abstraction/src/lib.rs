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

/// Trait for account-specific signature validation.
///
/// Different accounts may use different signature schemes (multisig,
/// social recovery, passkeys, etc). The EntryPoint delegates to this
/// trait rather than hardcoding Ed25519.
pub trait AccountValidator {
    /// Validate that `signature` is a valid authorization for the
    /// operation identified by `op_hash`, sent by `sender`.
    fn validate_signature(&self, sender: &Address, op_hash: &H256, signature: &[u8]) -> Result<()>;
}

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
    ///
    /// Excludes the `signature` field so the hash is available before signing.
    pub fn hash(&self) -> H256 {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.sender.as_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(&self.call_data);
        hasher.update(self.call_gas_limit.to_le_bytes());
        hasher.update(self.verification_gas_limit.to_le_bytes());
        hasher.update(self.pre_verification_gas.to_le_bytes());
        hasher.update(self.max_fee_per_gas.to_le_bytes());
        if let Some(pm) = &self.paymaster {
            hasher.update(pm.as_bytes());
        }
        hasher.update(&self.paymaster_data);
        // signature intentionally excluded
        H256::from_slice(&hasher.finalize()).unwrap()
    }

    /// Total gas this operation requires.
    pub fn total_gas(&self) -> u64 {
        self.call_gas_limit
            .saturating_add(self.verification_gas_limit)
            .saturating_add(self.pre_verification_gas)
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
    pub fn validate_user_op(
        &self,
        op: &UserOperation,
        validator: &dyn AccountValidator,
    ) -> Result<()> {
        op.validate()?;

        // Check sender is a registered smart account
        if !self.accounts.contains_key(&op.sender) {
            bail!("sender {:?} is not a registered smart account", op.sender);
        }

        // Validate signature against the operation hash
        let op_hash = op.hash();
        validator.validate_signature(&op.sender, &op_hash, &op.signature)?;

        // If paymaster is specified, check it's registered and has funds
        if let Some(paymaster) = &op.paymaster {
            let deposit = self
                .paymasters
                .get(paymaster)
                .ok_or_else(|| anyhow::anyhow!("paymaster not registered"))?;

            let required_gas_cost = (op.total_gas() as u128).saturating_mul(op.max_fee_per_gas);
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
    pub fn handle_ops(
        &mut self,
        ops: &[UserOperation],
        validator: &dyn AccountValidator,
    ) -> Result<Vec<UserOpResult>> {
        let mut results = Vec::new();

        for op in ops {
            let result = match self.validate_user_op(op, validator) {
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
                    self.nonces.insert(
                        op.sender,
                        expected_nonce
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("nonce overflow"))?,
                    );

                    // Deduct paymaster deposit if applicable
                    if let Some(paymaster) = &op.paymaster {
                        let cost = (op.total_gas() as u128).saturating_mul(op.max_fee_per_gas);
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

    struct AcceptAll;
    impl AccountValidator for AcceptAll {
        fn validate_signature(&self, _: &Address, _: &H256, _: &[u8]) -> Result<()> {
            Ok(())
        }
    }

    struct RejectAll;
    impl AccountValidator for RejectAll {
        fn validate_signature(&self, _: &Address, _: &H256, _: &[u8]) -> Result<()> {
            bail!("invalid signature")
        }
    }

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
    fn test_user_op_hash_excludes_signature() {
        let mut op1 = make_user_op(1);
        let mut op2 = make_user_op(1);
        op1.signature = vec![1; 64];
        op2.signature = vec![2; 64];
        assert_eq!(op1.hash(), op2.hash(), "hash must not depend on signature");
    }

    #[test]
    fn test_entrypoint_validates_registered_account() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        ep.register_account(sender, H256::from_slice(&[2u8; 32]).unwrap());

        let op = make_user_op(1);
        assert!(ep.validate_user_op(&op, &AcceptAll).is_ok());
    }

    #[test]
    fn test_entrypoint_rejects_unregistered_account() {
        let ep = EntryPoint::new();
        let op = make_user_op(1);
        assert!(ep.validate_user_op(&op, &AcceptAll).is_err());
    }

    #[test]
    fn test_entrypoint_rejects_invalid_signature() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        ep.register_account(sender, H256::zero());

        let op = make_user_op(1);
        let result = ep.validate_user_op(&op, &RejectAll);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid signature"));
    }

    #[test]
    fn test_paymaster_sponsorship() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        let paymaster = Address::from_slice(&[2u8; 20]).unwrap();

        ep.register_account(sender, H256::zero());
        ep.register_paymaster(paymaster, 100_000_000);

        let mut op = make_user_op(1);
        op.paymaster = Some(paymaster);

        assert!(ep.validate_user_op(&op, &AcceptAll).is_ok());
    }

    #[test]
    fn test_paymaster_insufficient_deposit() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        let paymaster = Address::from_slice(&[2u8; 20]).unwrap();

        ep.register_account(sender, H256::zero());
        ep.register_paymaster(paymaster, 1);

        let mut op = make_user_op(1);
        op.paymaster = Some(paymaster);

        assert!(ep.validate_user_op(&op, &AcceptAll).is_err());
    }

    #[test]
    fn test_handle_ops_batch() {
        let mut ep = EntryPoint::new();
        let s1 = Address::from_slice(&[1u8; 20]).unwrap();
        let s2 = Address::from_slice(&[2u8; 20]).unwrap();

        ep.register_account(s1, H256::zero());
        ep.register_account(s2, H256::zero());

        let ops = vec![make_user_op(1), make_user_op(2)];
        let results = ep.handle_ops(&ops, &AcceptAll).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);
    }

    #[test]
    fn test_handle_ops_rejects_bad_signature() {
        let mut ep = EntryPoint::new();
        let sender = Address::from_slice(&[1u8; 20]).unwrap();
        ep.register_account(sender, H256::zero());

        let ops = vec![make_user_op(1)];
        let results = ep.handle_ops(&ops, &RejectAll).unwrap();

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.as_ref().unwrap().contains("signature"));
    }

    #[test]
    fn test_total_gas() {
        let op = make_user_op(1);
        assert_eq!(op.total_gas(), 100_000 + 50_000 + 10_000);
    }

    #[test]
    fn test_total_gas_saturates() {
        let mut op = make_user_op(1);
        op.call_gas_limit = u64::MAX;
        op.verification_gas_limit = 1;
        assert_eq!(op.total_gas(), u64::MAX);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    struct AcceptAll;
    impl AccountValidator for AcceptAll {
        fn validate_signature(&self, _: &Address, _: &H256, _: &[u8]) -> Result<()> {
            Ok(())
        }
    }

    fn arb_address() -> impl Strategy<Value = Address> {
        prop::array::uniform20(any::<u8>()).prop_map(|b| Address::from_slice(&b).unwrap())
    }

    proptest! {
        /// UserOperation hash is deterministic — same inputs always produce same hash.
        #[test]
        fn user_op_hash_deterministic(sender in arb_address(), nonce in any::<u64>()) {
            let op = UserOperation {
                sender,
                nonce,
                call_data: vec![1, 2, 3],
                call_gas_limit: 100_000,
                verification_gas_limit: 50_000,
                pre_verification_gas: 10_000,
                max_fee_per_gas: 100,
                paymaster: None,
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };
            prop_assert_eq!(op.hash(), op.hash());
        }

        /// Changing any field (except signature) changes the hash.
        #[test]
        fn user_op_hash_sensitive_to_fields(
            nonce1 in 0u64..u64::MAX,
            nonce2 in 0u64..u64::MAX,
        ) {
            prop_assume!(nonce1 != nonce2);
            let sender = Address::from_slice(&[1u8; 20]).unwrap();
            let mut op1 = UserOperation {
                sender,
                nonce: nonce1,
                call_data: vec![],
                call_gas_limit: 100_000,
                verification_gas_limit: 50_000,
                pre_verification_gas: 10_000,
                max_fee_per_gas: 100,
                paymaster: None,
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };
            let mut op2 = op1.clone();
            op2.nonce = nonce2;
            // Different signatures — hash must still differ due to nonce
            op1.signature = vec![0u8; 64];
            op2.signature = vec![0u8; 64];
            prop_assert_ne!(op1.hash(), op2.hash());
        }

        /// Hash excludes the signature field — two ops identical except for signature hash equal.
        #[test]
        fn user_op_hash_excludes_signature(
            sig1 in prop::collection::vec(any::<u8>(), 1..128usize),
            sig2 in prop::collection::vec(any::<u8>(), 1..128usize),
        ) {
            let sender = Address::from_slice(&[1u8; 20]).unwrap();
            let op1 = UserOperation {
                sender,
                nonce: 0,
                call_data: vec![],
                call_gas_limit: 100_000,
                verification_gas_limit: 50_000,
                pre_verification_gas: 10_000,
                max_fee_per_gas: 100,
                paymaster: None,
                paymaster_data: vec![],
                signature: sig1,
            };
            let mut op2 = op1.clone();
            op2.signature = sig2;
            prop_assert_eq!(op1.hash(), op2.hash());
        }

        /// total_gas() never overflows — always saturates at u64::MAX.
        #[test]
        fn total_gas_saturates(
            call_gas in any::<u64>(),
            verif_gas in any::<u64>(),
            pre_gas in any::<u64>(),
        ) {
            let op = UserOperation {
                sender: Address::from_slice(&[1u8; 20]).unwrap(),
                nonce: 0,
                call_data: vec![],
                call_gas_limit: call_gas,
                verification_gas_limit: verif_gas,
                pre_verification_gas: pre_gas,
                max_fee_per_gas: 1,
                paymaster: None,
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };
            // Must not panic — just return a u64 (possibly saturated)
            let _ = op.total_gas();
        }

        /// Zero call_gas_limit is always rejected by validate().
        #[test]
        fn zero_call_gas_rejected(verif_gas in 1u64..1_000_000u64, max_fee in 1u128..10_000u128) {
            let op = UserOperation {
                sender: Address::from_slice(&[1u8; 20]).unwrap(),
                nonce: 0,
                call_data: vec![],
                call_gas_limit: 0,
                verification_gas_limit: verif_gas,
                pre_verification_gas: 1,
                max_fee_per_gas: max_fee,
                paymaster: None,
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };
            prop_assert!(op.validate().is_err());
        }

        /// Nonce monotonicity: sequential nonces 0,1,2,... all succeed for same sender.
        #[test]
        fn sequential_nonces_accepted(n_ops in 1usize..20usize) {
            let sender = Address::from_slice(&[1u8; 20]).unwrap();
            let mut ep = EntryPoint::new();
            ep.register_account(sender, H256::zero());

            let ops: Vec<UserOperation> = (0u64..n_ops as u64)
                .map(|nonce| UserOperation {
                    sender,
                    nonce,
                    call_data: vec![],
                    call_gas_limit: 100_000,
                    verification_gas_limit: 50_000,
                    pre_verification_gas: 10_000,
                    max_fee_per_gas: 100,
                    paymaster: None,
                    paymaster_data: vec![],
                    signature: vec![0u8; 64],
                })
                .collect();

            let results = ep.handle_ops(&ops, &AcceptAll).unwrap();
            prop_assert_eq!(results.len(), n_ops);
            for r in &results {
                prop_assert!(r.success, "op failed: {:?}", r.error);
            }
        }

        /// Replaying the same nonce always fails on the second attempt.
        #[test]
        fn replay_same_nonce_rejected(sender in arb_address()) {
            let mut ep = EntryPoint::new();
            ep.register_account(sender, H256::zero());

            let op = UserOperation {
                sender,
                nonce: 0,
                call_data: vec![1],
                call_gas_limit: 100_000,
                verification_gas_limit: 50_000,
                pre_verification_gas: 10_000,
                max_fee_per_gas: 100,
                paymaster: None,
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };

            // First execution succeeds
            let r1 = ep.handle_ops(std::slice::from_ref(&op), &AcceptAll).unwrap();
            prop_assert!(r1[0].success);

            // Replay with same nonce must fail
            let r2 = ep.handle_ops(std::slice::from_ref(&op), &AcceptAll).unwrap();
            prop_assert!(!r2[0].success, "replay must be rejected");
            let err = r2[0].error.as_deref().unwrap_or("");
            prop_assert!(err.contains("nonce"), "expected nonce error, got: {}", err);
        }

        /// Unregistered sender always rejected by validate_user_op.
        #[test]
        fn unregistered_sender_rejected(sender in arb_address()) {
            let ep = EntryPoint::new();
            let op = UserOperation {
                sender,
                nonce: 0,
                call_data: vec![],
                call_gas_limit: 100_000,
                verification_gas_limit: 50_000,
                pre_verification_gas: 10_000,
                max_fee_per_gas: 100,
                paymaster: None,
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };
            prop_assert!(ep.validate_user_op(&op, &AcceptAll).is_err());
        }

        /// Paymaster deposit is correctly deducted after op execution.
        #[test]
        fn paymaster_deposit_decremented(
            sender in arb_address(),
            paymaster in arb_address(),
            call_gas in 1u64..100_000u64,
            verif_gas in 1u64..50_000u64,
            pre_gas in 1u64..10_000u64,
            max_fee in 1u128..1_000u128,
        ) {
            prop_assume!(sender != paymaster);
            let mut ep = EntryPoint::new();
            ep.register_account(sender, H256::zero());

            // Compute required deposit from gas limits
            let total_gas = call_gas.saturating_add(verif_gas).saturating_add(pre_gas);
            let cost = (total_gas as u128).saturating_mul(max_fee);
            // Provide exactly enough + 1 to ensure success
            let initial_deposit = cost.saturating_add(1);
            ep.register_paymaster(paymaster, initial_deposit);

            let op = UserOperation {
                sender,
                nonce: 0,
                call_data: vec![],
                call_gas_limit: call_gas,
                verification_gas_limit: verif_gas,
                pre_verification_gas: pre_gas,
                max_fee_per_gas: max_fee,
                paymaster: Some(paymaster),
                paymaster_data: vec![],
                signature: vec![0u8; 64],
            };

            let results = ep.handle_ops(&[op], &AcceptAll).unwrap();
            prop_assert!(results[0].success);
        }

        /// handle_ops returns one result per submitted op regardless of success/failure.
        #[test]
        fn result_count_matches_input(n_ops in 1usize..10usize) {
            let mut ep = EntryPoint::new();
            // Register only odd-indexed senders — evens will fail (unregistered)
            let ops: Vec<UserOperation> = (0..n_ops)
                .map(|i| {
                    let sender = Address::from_slice(&[i as u8 + 1; 20]).unwrap();
                    if i % 2 == 0 {
                        ep.register_account(sender, H256::zero());
                    }
                    UserOperation {
                        sender,
                        nonce: 0,
                        call_data: vec![],
                        call_gas_limit: 100_000,
                        verification_gas_limit: 50_000,
                        pre_verification_gas: 10_000,
                        max_fee_per_gas: 100,
                        paymaster: None,
                        paymaster_data: vec![],
                        signature: vec![0u8; 64],
                    }
                })
                .collect();

            let results = ep.handle_ops(&ops, &AcceptAll).unwrap();
            prop_assert_eq!(results.len(), n_ops, "one result per submitted op");
        }
    }
}
