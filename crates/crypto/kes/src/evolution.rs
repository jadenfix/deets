use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{KesError, Result};
use crate::signature::{KesSignature, KesVerificationKey};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KesKey {
    seed: [u8; 32],
    root: [u8; 32],
    current_period: u32,
    max_periods: u32,
}

impl KesKey {
    /// Create a new key from an explicit seed.
    pub fn from_seed(seed: [u8; 32], max_periods: u32) -> Self {
        let root = Self::derive_root(&seed);
        KesKey {
            seed,
            root,
            current_period: 0,
            max_periods,
        }
    }

    /// Generate a key using secure randomness.
    pub fn generate(max_periods: u32) -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        Self::from_seed(seed, max_periods)
    }

    /// Maximum number of supported periods.
    pub fn max_periods(&self) -> u32 {
        self.max_periods
    }

    /// Current evolved period.
    pub fn current_period(&self) -> u32 {
        self.current_period
    }

    /// Public verification key associated with this KES key.
    pub fn verification_key(&self) -> KesVerificationKey {
        KesVerificationKey::new(self.root, self.max_periods)
    }

    /// Sign a message for the provided period.
    pub fn sign(&mut self, period: u32, message: &[u8]) -> Result<KesSignature> {
        if period >= self.max_periods {
            return Err(KesError::PeriodOutOfRange {
                requested: period,
                max_periods: self.max_periods,
            });
        }

        if period < self.current_period {
            return Err(KesError::PeriodRegression {
                current: self.current_period,
                requested: period,
            });
        }

        self.current_period = period;

        let vk = self.verification_key();
        let period_tag = vk.derive_period_tag(period);
        let signature_bytes = vk.derive_signature(&period_tag, message);

        Ok(KesSignature::new(period, period_tag, signature_bytes))
    }

    fn derive_root(seed: &[u8; 32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(seed);
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_signs() {
        let mut key = KesKey::generate(16);
        let vk = key.verification_key();

        let sig = key.sign(0, b"hello world").unwrap();
        assert!(sig.verify(&vk, b"hello world"));
        assert!(!sig.verify(&vk, b"different"));
        assert_eq!(key.current_period(), 0);
    }

    #[test]
    fn monotonic_period() {
        let mut key = KesKey::generate(4);
        key.sign(1, b"test").unwrap();
        let err = key.sign(0, b"regress").unwrap_err();
        assert_eq!(
            err,
            KesError::PeriodRegression {
                current: 1,
                requested: 0,
            }
        );
    }

    #[test]
    fn bounds_check() {
        let mut key = KesKey::generate(2);
        key.sign(0, b"test").unwrap();
        let err = key.sign(2, b"oob").unwrap_err();
        assert_eq!(
            err,
            KesError::PeriodOutOfRange {
                requested: 2,
                max_periods: 2,
            }
        );
    }
}
