use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KesSignature {
    pub period: u32,
    pub period_tag: [u8; 32],
    pub signature: [u8; 32],
}

impl KesSignature {
    pub(crate) fn new(period: u32, period_tag: [u8; 32], signature: [u8; 32]) -> Self {
        KesSignature {
            period,
            period_tag,
            signature,
        }
    }

    /// Verify the signature against the provided verification key and message.
    pub fn verify(&self, vk: &KesVerificationKey, message: &[u8]) -> bool {
        if self.period >= vk.max_periods {
            return false;
        }

        let expected_tag = vk.derive_period_tag(self.period);
        if expected_tag != self.period_tag {
            return false;
        }

        let expected_sig = vk.derive_signature(&self.period_tag, message);
        expected_sig == self.signature
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KesVerificationKey {
    root: [u8; 32],
    max_periods: u32,
}

impl KesVerificationKey {
    pub fn new(root: [u8; 32], max_periods: u32) -> Self {
        KesVerificationKey { root, max_periods }
    }

    pub fn root(&self) -> [u8; 32] {
        self.root
    }

    pub fn max_periods(&self) -> u32 {
        self.max_periods
    }

    pub(crate) fn derive_period_tag(&self, period: u32) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.root);
        hasher.update(period.to_be_bytes());
        hasher.finalize().into()
    }

    pub(crate) fn derive_signature(&self, period_tag: &[u8; 32], message: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(period_tag);
        hasher.update(message);
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_roundtrip() {
        let vk = KesVerificationKey::new([1u8; 32], 8);
        let period_tag = vk.derive_period_tag(3);
        let sig_bytes = vk.derive_signature(&period_tag, b"msg");
        let sig = KesSignature::new(3, period_tag, sig_bytes);

        assert!(sig.verify(&vk, b"msg"));
        assert!(!sig.verify(&vk, b"other"));
    }
}
