use aether_types::{Address, Transaction, H256};
use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// A commitment to a transaction (submitted during the commit phase).
///
/// Contains only the hash of the encrypted transaction, so the proposer
/// cannot see or reorder based on content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransactionCommitment {
    /// SHA-256(encrypted_tx_bytes || salt)
    pub commitment_hash: H256,
    /// Who submitted the commitment.
    pub sender: Address,
    /// Slot in which this commitment was included.
    pub commit_slot: u64,
    /// Fee prepaid for commitment inclusion.
    pub commit_fee: u128,
}

/// A revealed transaction that matches a prior commitment.
#[derive(Debug, Clone)]
pub struct RevealedTransaction {
    /// The actual transaction.
    pub transaction: Transaction,
    /// The salt used in the commitment hash.
    pub salt: [u8; 32],
    /// The commitment this reveal matches.
    pub commitment_hash: H256,
}

/// Manages the commit-reveal lifecycle for MEV-resistant ordering.
pub struct CommitRevealPool {
    /// Pending commitments: commitment_hash → commitment
    commitments: HashMap<H256, TransactionCommitment>,
    /// Revealed transactions: commitment_hash → revealed tx
    reveals: HashMap<H256, RevealedTransaction>,
    /// How many slots after commit before reveal is accepted.
    reveal_delay: u64,
    /// How many slots after commit before commitment expires.
    commitment_ttl: u64,
}

impl CommitRevealPool {
    pub fn new(reveal_delay: u64, commitment_ttl: u64) -> Self {
        CommitRevealPool {
            commitments: HashMap::new(),
            reveals: HashMap::new(),
            reveal_delay,
            commitment_ttl,
        }
    }

    /// Create a commitment hash for a transaction.
    ///
    /// commitment_hash = SHA-256(serialize(tx) || salt)
    pub fn create_commitment(tx: &Transaction, salt: &[u8; 32]) -> Result<H256> {
        let tx_bytes = bincode::serialize(tx)
            .map_err(|e| anyhow::anyhow!("transaction serialization failed: {}", e))?;
        let mut hasher = Sha256::new();
        hasher.update(&tx_bytes);
        hasher.update(salt);
        Ok(H256::from_slice(&hasher.finalize()).expect("SHA256 always produces 32 bytes"))
    }

    /// Submit a commitment (commit phase).
    pub fn submit_commitment(&mut self, commitment: TransactionCommitment) -> Result<()> {
        if self.commitments.contains_key(&commitment.commitment_hash) {
            bail!("duplicate commitment");
        }
        self.commitments
            .insert(commitment.commitment_hash, commitment);
        Ok(())
    }

    /// Reveal a transaction that matches a prior commitment (reveal phase).
    pub fn reveal(&mut self, tx: Transaction, salt: [u8; 32], current_slot: u64) -> Result<()> {
        // Compute what the commitment hash should be
        let expected_hash = Self::create_commitment(&tx, &salt)?;

        // Find the matching commitment
        let commitment = self
            .commitments
            .get(&expected_hash)
            .ok_or_else(|| anyhow::anyhow!("no matching commitment found"))?;

        // Check reveal timing: must be after reveal_delay
        if current_slot < commitment.commit_slot + self.reveal_delay {
            bail!(
                "reveal too early: current slot {} < commit slot {} + delay {}",
                current_slot,
                commitment.commit_slot,
                self.reveal_delay
            );
        }

        // Check commitment hasn't expired
        if current_slot > commitment.commit_slot + self.commitment_ttl {
            bail!("commitment expired");
        }

        // Check sender matches
        if tx.sender != commitment.sender {
            bail!("reveal sender does not match commitment sender");
        }

        // Store the revealed transaction
        self.reveals.insert(
            expected_hash,
            RevealedTransaction {
                transaction: tx,
                salt,
                commitment_hash: expected_hash,
            },
        );

        Ok(())
    }

    /// Get all revealed transactions ready for execution.
    /// Returns them in deterministic order: sorted by commitment slot,
    /// then by commitment hash for tie-breaking. This ensures all validators
    /// produce identical transaction ordering (HashMap iteration is undefined).
    pub fn get_revealed_transactions(&self) -> Vec<&RevealedTransaction> {
        let mut revealed: Vec<&RevealedTransaction> = self.reveals.values().collect();
        revealed.sort_by(|a, b| {
            let slot_a = self
                .commitments
                .get(&a.commitment_hash)
                .map_or(0, |c| c.commit_slot);
            let slot_b = self
                .commitments
                .get(&b.commitment_hash)
                .map_or(0, |c| c.commit_slot);
            slot_a
                .cmp(&slot_b)
                .then_with(|| a.commitment_hash.0.cmp(&b.commitment_hash.0))
        });
        revealed
    }

    /// Clean up expired commitments.
    pub fn cleanup_expired(&mut self, current_slot: u64) {
        self.commitments
            .retain(|_, c| current_slot <= c.commit_slot + self.commitment_ttl);
    }

    /// Remove a commitment + reveal after execution.
    pub fn remove(&mut self, commitment_hash: &H256) {
        self.commitments.remove(commitment_hash);
        self.reveals.remove(commitment_hash);
    }

    pub fn pending_commitments(&self) -> usize {
        self.commitments.len()
    }

    pub fn pending_reveals(&self) -> usize {
        self.reveals.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::*;

    fn make_tx(sender_byte: u8, nonce: u64) -> Transaction {
        Transaction {
            nonce,
            chain_id: 1,
            sender: Address::from_slice(&[sender_byte; 20]).unwrap(),
            sender_pubkey: PublicKey::from_bytes(vec![sender_byte; 32]),
            inputs: vec![],
            outputs: vec![],
            reads: std::collections::HashSet::new(),
            writes: std::collections::HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21000,
            fee: 1000,
            signature: Signature::from_bytes(vec![0u8; 64]),
        }
    }

    #[test]
    fn test_commit_reveal_roundtrip() {
        let mut pool = CommitRevealPool::new(2, 100);

        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        // Commit at slot 10
        pool.submit_commitment(TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        })
        .unwrap();

        assert_eq!(pool.pending_commitments(), 1);

        // Reveal at slot 12 (after delay of 2)
        pool.reveal(tx, salt, 12).unwrap();
        assert_eq!(pool.pending_reveals(), 1);

        let revealed = pool.get_revealed_transactions();
        assert_eq!(revealed.len(), 1);
        assert_eq!(revealed[0].commitment_hash, hash);
    }

    #[test]
    fn test_reject_early_reveal() {
        let mut pool = CommitRevealPool::new(5, 100);

        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        pool.submit_commitment(TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        })
        .unwrap();

        // Try to reveal too early (slot 12 < 10 + 5 = 15)
        let result = pool.reveal(tx, salt, 12);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too early"));
    }

    #[test]
    fn test_reject_expired_commitment() {
        let mut pool = CommitRevealPool::new(2, 10);

        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        pool.submit_commitment(TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        })
        .unwrap();

        // Try to reveal after expiry (slot 25 > 10 + 10 = 20)
        let result = pool.reveal(tx, salt, 25);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expired"));
    }

    #[test]
    fn test_reject_wrong_salt() {
        let mut pool = CommitRevealPool::new(2, 100);

        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        pool.submit_commitment(TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        })
        .unwrap();

        // Reveal with wrong salt
        let wrong_salt = [99u8; 32];
        let result = pool.reveal(tx, wrong_salt, 12);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no matching"));
    }

    #[test]
    fn test_reject_sender_mismatch() {
        let mut pool = CommitRevealPool::new(2, 100);

        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        pool.submit_commitment(TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        })
        .unwrap();

        // Try to reveal with different sender
        let mut fake_tx = tx.clone();
        fake_tx.sender = Address::from_slice(&[99u8; 20]).unwrap();
        // Need to recompute hash with same salt for the fake tx to even match
        // Since the hash won't match, this should fail with "no matching"
        let result = pool.reveal(fake_tx, salt, 12);
        assert!(result.is_err());
    }

    #[test]
    fn test_cleanup_expired() {
        let mut pool = CommitRevealPool::new(2, 10);

        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        pool.submit_commitment(TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        })
        .unwrap();

        assert_eq!(pool.pending_commitments(), 1);

        // Cleanup at slot 25 (> 10 + 10 = 20, expired)
        pool.cleanup_expired(25);
        assert_eq!(pool.pending_commitments(), 0);
    }

    #[test]
    fn test_commitment_hash_deterministic() {
        let tx = make_tx(1, 0);
        let salt = [42u8; 32];

        let h1 = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
        let h2 = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_duplicate_commitment_rejected() {
        let mut pool = CommitRevealPool::new(2, 100);
        let tx = make_tx(1, 0);
        let salt = [42u8; 32];
        let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

        let commitment = TransactionCommitment {
            commitment_hash: hash,
            sender: tx.sender,
            commit_slot: 10,
            commit_fee: 1000,
        };

        pool.submit_commitment(commitment.clone()).unwrap();
        let result = pool.submit_commitment(commitment);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn test_revealed_transactions_deterministic_order() {
        // Insert multiple reveals and verify ordering is deterministic
        // (sorted by commit_slot, then commitment_hash), not HashMap-random.
        let mut pool = CommitRevealPool::new(1, 100);

        // Create 5 transactions committed at different slots
        let salts: [[u8; 32]; 5] = [[10u8; 32], [20u8; 32], [30u8; 32], [40u8; 32], [50u8; 32]];
        let commit_slots = [15u64, 10, 20, 10, 12]; // intentionally unordered, with ties

        let mut hashes = Vec::new();
        for i in 0..5 {
            let tx = make_tx((i + 1) as u8, i as u64);
            let hash = CommitRevealPool::create_commitment(&tx, &salts[i]).unwrap();
            hashes.push(hash);

            pool.submit_commitment(TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot: commit_slots[i],
                commit_fee: 1000,
            })
            .unwrap();

            // Reveal after delay
            pool.reveal(tx, salts[i], commit_slots[i] + 1).unwrap();
        }

        // Get revealed transactions multiple times — must be identical each time
        let first_order: Vec<H256> = pool
            .get_revealed_transactions()
            .iter()
            .map(|r| r.commitment_hash)
            .collect();

        for _ in 0..10 {
            let order: Vec<H256> = pool
                .get_revealed_transactions()
                .iter()
                .map(|r| r.commitment_hash)
                .collect();
            assert_eq!(first_order, order, "ordering must be deterministic");
        }

        // Verify slot ordering: each commit_slot must be <= the next
        let revealed = pool.get_revealed_transactions();
        for i in 1..revealed.len() {
            let slot_prev = pool
                .commitments
                .get(&revealed[i - 1].commitment_hash)
                .unwrap()
                .commit_slot;
            let slot_curr = pool
                .commitments
                .get(&revealed[i].commitment_hash)
                .unwrap()
                .commit_slot;
            assert!(
                slot_prev <= slot_curr,
                "revealed txs must be sorted by commit_slot: {} > {}",
                slot_prev,
                slot_curr
            );
            // Within same slot, sorted by hash
            if slot_prev == slot_curr {
                assert!(
                    revealed[i - 1].commitment_hash.0 <= revealed[i].commitment_hash.0,
                    "same-slot txs must be sorted by commitment_hash"
                );
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_types::*;
    use proptest::prelude::*;

    fn arb_salt() -> impl Strategy<Value = [u8; 32]> {
        prop::array::uniform32(any::<u8>())
    }

    fn arb_tx(sender_byte: u8, nonce: u64) -> Transaction {
        Transaction {
            nonce,
            chain_id: 1,
            sender: Address::from_slice(&[sender_byte; 20]).unwrap(),
            sender_pubkey: PublicKey::from_bytes(vec![sender_byte; 32]),
            inputs: vec![],
            outputs: vec![],
            reads: std::collections::HashSet::new(),
            writes: std::collections::HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 21000,
            fee: 1000,
            signature: Signature::from_bytes(vec![0u8; 64]),
        }
    }

    proptest! {
        /// Commitment hash is deterministic: same tx + salt always yields same hash.
        #[test]
        fn commitment_hash_deterministic(
            sender_byte in 1u8..=255,
            nonce in 0u64..1000,
            salt in arb_salt(),
        ) {
            let tx = arb_tx(sender_byte, nonce);
            let h1 = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
            let h2 = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
            prop_assert_eq!(h1, h2);
        }

        /// Different salts produce different commitment hashes (collision resistance).
        #[test]
        fn different_salts_different_hashes(
            sender_byte in 1u8..=255,
            nonce in 0u64..1000,
            salt_a in arb_salt(),
            salt_b in arb_salt(),
        ) {
            prop_assume!(salt_a != salt_b);
            let tx = arb_tx(sender_byte, nonce);
            let h1 = CommitRevealPool::create_commitment(&tx, &salt_a).unwrap();
            let h2 = CommitRevealPool::create_commitment(&tx, &salt_b).unwrap();
            prop_assert_ne!(h1, h2);
        }

        /// Different transactions with the same salt produce different hashes.
        #[test]
        fn different_txs_different_hashes(
            sender_a in 1u8..=127,
            sender_b in 128u8..=255,
            salt in arb_salt(),
        ) {
            let tx_a = arb_tx(sender_a, 0);
            let tx_b = arb_tx(sender_b, 0);
            let h1 = CommitRevealPool::create_commitment(&tx_a, &salt).unwrap();
            let h2 = CommitRevealPool::create_commitment(&tx_b, &salt).unwrap();
            prop_assert_ne!(h1, h2);
        }

        /// A valid commit-reveal roundtrip always succeeds when timing is correct.
        #[test]
        fn valid_roundtrip_succeeds(
            sender_byte in 1u8..=255,
            nonce in 0u64..1000,
            salt in arb_salt(),
            reveal_delay in 1u64..10,
            commit_slot in 0u64..1000,
        ) {
            let ttl = 100u64;
            let mut pool = CommitRevealPool::new(reveal_delay, ttl);
            let tx = arb_tx(sender_byte, nonce);
            let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

            pool.submit_commitment(TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot,
                commit_fee: 1000,
            }).unwrap();

            let reveal_slot = commit_slot.saturating_add(reveal_delay);
            pool.reveal(tx, salt, reveal_slot).unwrap();
            prop_assert_eq!(pool.pending_reveals(), 1);
        }

        /// Reveal before delay always fails.
        #[test]
        fn early_reveal_rejected(
            sender_byte in 1u8..=255,
            salt in arb_salt(),
            reveal_delay in 2u64..20,
            commit_slot in 10u64..1000,
            early_offset in 0u64..10,
        ) {
            let early_slot = commit_slot.saturating_add(early_offset.min(reveal_delay - 1));
            prop_assume!(early_slot < commit_slot.saturating_add(reveal_delay));

            let mut pool = CommitRevealPool::new(reveal_delay, 1000);
            let tx = arb_tx(sender_byte, 0);
            let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

            pool.submit_commitment(TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot,
                commit_fee: 1000,
            }).unwrap();

            let result = pool.reveal(tx, salt, early_slot);
            prop_assert!(result.is_err());
        }

        /// Reveal after TTL always fails with expiry.
        #[test]
        fn expired_reveal_rejected(
            sender_byte in 1u8..=255,
            salt in arb_salt(),
            commit_slot in 0u64..100,
            ttl in 5u64..50,
            extra in 1u64..100,
        ) {
            let mut pool = CommitRevealPool::new(1, ttl);
            let tx = arb_tx(sender_byte, 0);
            let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

            pool.submit_commitment(TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot,
                commit_fee: 1000,
            }).unwrap();

            let expired_slot = commit_slot.saturating_add(ttl).saturating_add(extra);
            let result = pool.reveal(tx, salt, expired_slot);
            prop_assert!(result.is_err());
        }

        /// cleanup_expired removes exactly the expired commitments.
        #[test]
        fn cleanup_removes_only_expired(
            commit_slots in prop::collection::vec(0u64..100, 1..10),
            ttl in 5u64..20,
            cleanup_slot in 0u64..200,
        ) {
            let mut pool = CommitRevealPool::new(1, ttl);
            let mut expected_remaining = 0usize;

            for (i, &slot) in commit_slots.iter().enumerate() {
                let tx = arb_tx((i as u8).wrapping_add(1), i as u64);
                let salt = [i as u8; 32];
                let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

                pool.submit_commitment(TransactionCommitment {
                    commitment_hash: hash,
                    sender: tx.sender,
                    commit_slot: slot,
                    commit_fee: 1000,
                }).unwrap();

                if cleanup_slot <= slot.saturating_add(ttl) {
                    expected_remaining += 1;
                }
            }

            pool.cleanup_expired(cleanup_slot);
            prop_assert_eq!(pool.pending_commitments(), expected_remaining);
        }

        /// Revealed transactions are always sorted by commit_slot then hash.
        #[test]
        fn revealed_order_invariant(
            count in 2usize..8,
        ) {
            let mut pool = CommitRevealPool::new(1, 1000);

            for i in 0..count {
                let tx = arb_tx((i as u8).wrapping_add(1), i as u64);
                let salt = [i as u8; 32];
                let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
                let commit_slot = (i as u64) % 3; // create slot ties

                pool.submit_commitment(TransactionCommitment {
                    commitment_hash: hash,
                    sender: tx.sender,
                    commit_slot,
                    commit_fee: 1000,
                }).unwrap();

                pool.reveal(tx, salt, commit_slot + 1).unwrap();
            }

            let revealed = pool.get_revealed_transactions();
            for w in revealed.windows(2) {
                let slot_a = pool.commitments.get(&w[0].commitment_hash).unwrap().commit_slot;
                let slot_b = pool.commitments.get(&w[1].commitment_hash).unwrap().commit_slot;
                prop_assert!(slot_a <= slot_b);
                if slot_a == slot_b {
                    prop_assert!(w[0].commitment_hash.0 <= w[1].commitment_hash.0);
                }
            }
        }

        /// remove() cleans up both commitment and reveal.
        #[test]
        fn remove_clears_both(
            sender_byte in 1u8..=255,
            salt in arb_salt(),
        ) {
            let mut pool = CommitRevealPool::new(1, 100);
            let tx = arb_tx(sender_byte, 0);
            let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

            pool.submit_commitment(TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot: 0,
                commit_fee: 1000,
            }).unwrap();
            pool.reveal(tx, salt, 1).unwrap();

            prop_assert_eq!(pool.pending_commitments(), 1);
            prop_assert_eq!(pool.pending_reveals(), 1);

            pool.remove(&hash);
            prop_assert_eq!(pool.pending_commitments(), 0);
            prop_assert_eq!(pool.pending_reveals(), 0);
        }

        /// Duplicate commitment submission is always rejected.
        #[test]
        fn duplicate_commitment_rejected(
            sender_byte in 1u8..=255,
            salt in arb_salt(),
        ) {
            let mut pool = CommitRevealPool::new(1, 100);
            let tx = arb_tx(sender_byte, 0);
            let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();

            let commitment = TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot: 0,
                commit_fee: 1000,
            };

            pool.submit_commitment(commitment.clone()).unwrap();
            prop_assert!(pool.submit_commitment(commitment).is_err());
        }
    }
}
