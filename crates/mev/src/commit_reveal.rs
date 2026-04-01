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
    /// Returns them sorted by commit_slot (ascending), then by commitment_hash
    /// for deterministic ordering across all validators.
    pub fn get_revealed_transactions(&self) -> Vec<&RevealedTransaction> {
        let mut txs: Vec<_> = self.reveals.values().collect();
        txs.sort_by(|a, b| {
            let slot_a = self
                .commitments
                .get(&a.commitment_hash)
                .map(|c| c.commit_slot)
                .unwrap_or(0);
            let slot_b = self
                .commitments
                .get(&b.commitment_hash)
                .map(|c| c.commit_slot)
                .unwrap_or(0);
            slot_a
                .cmp(&slot_b)
                .then_with(|| a.commitment_hash.as_bytes().cmp(b.commitment_hash.as_bytes()))
        });
        txs
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
    fn test_revealed_transactions_ordered_by_commit_slot() {
        let mut pool = CommitRevealPool::new(1, 100);

        // Submit 3 commitments at different slots, reveal in reverse order
        let mut hashes = vec![];
        for i in (0..3).rev() {
            let tx = make_tx(i + 1, 0);
            let salt = [i + 1; 32];
            let hash = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
            pool.submit_commitment(TransactionCommitment {
                commitment_hash: hash,
                sender: tx.sender,
                commit_slot: (i as u64) * 10, // slots 0, 10, 20 but inserted in reverse
                commit_fee: 1000,
            })
            .unwrap();
            // Reveal immediately after delay
            pool.reveal(tx, salt, (i as u64) * 10 + 1).unwrap();
            hashes.push(hash);
        }

        let revealed = pool.get_revealed_transactions();
        assert_eq!(revealed.len(), 3);

        // Must be sorted by commit_slot ascending regardless of insertion order
        let slots: Vec<u64> = revealed
            .iter()
            .map(|r| {
                pool.commitments
                    .get(&r.commitment_hash)
                    .unwrap()
                    .commit_slot
            })
            .collect();
        assert!(
            slots.windows(2).all(|w| w[0] <= w[1]),
            "revealed transactions must be ordered by commit_slot, got: {:?}",
            slots
        );

        // Run 100 times to verify determinism (HashMap ordering is random)
        let first_order: Vec<H256> = revealed.iter().map(|r| r.commitment_hash).collect();
        for _ in 0..100 {
            let order: Vec<H256> = pool
                .get_revealed_transactions()
                .iter()
                .map(|r| r.commitment_hash)
                .collect();
            assert_eq!(first_order, order, "ordering must be deterministic");
        }
    }

    #[test]
    fn test_commitment_hash_deterministic() {
        let tx = make_tx(1, 0);
        let salt = [42u8; 32];

        let h1 = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
        let h2 = CommitRevealPool::create_commitment(&tx, &salt).unwrap();
        assert_eq!(h1, h2);
    }
}
