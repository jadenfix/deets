use aether_types::H256;
use serde::{Deserialize, Serialize};

use crate::error::{Result, VerifierError};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KzgChallenge {
    pub vcr_id: H256,
    pub layer_indices: Vec<u32>,
    pub point_indices: Vec<Vec<u32>>,
    pub deadline_slot: u64,
}

impl KzgChallenge {
    /// Validate the internal structure of the challenge.
    pub fn validate(&self) -> Result<()> {
        if self.layer_indices.len() != self.point_indices.len() {
            return Err(VerifierError::InvalidChallenge(
                "layer and point vectors must align",
            ));
        }

        if self.layer_indices.is_empty() {
            return Err(VerifierError::InvalidChallenge(
                "challenge must request at least one layer",
            ));
        }

        for points in &self.point_indices {
            if points.is_empty() {
                return Err(VerifierError::InvalidChallenge(
                    "each challenged layer must request at least one point",
                ));
            }
        }

        Ok(())
    }

    /// Total number of individual openings required by the challenge.
    pub fn expected_openings(&self) -> usize {
        self.point_indices.iter().map(|points| points.len()).sum()
    }

    /// Return whether a layer/point pair is part of the challenge.
    pub fn contains(&self, layer: u32, point: u32) -> bool {
        self.layer_indices
            .iter()
            .zip(&self.point_indices)
            .any(|(&layer_idx, points)| layer_idx == layer && points.contains(&point))
    }

    /// Iterator over all requested layer/point pairs.
    pub fn iter_points(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.layer_indices
            .iter()
            .copied()
            .zip(self.point_indices.iter())
            .flat_map(|(layer, points)| points.iter().copied().map(move |point| (layer, point)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::H256;

    fn sample_challenge() -> KzgChallenge {
        KzgChallenge {
            vcr_id: H256::zero(),
            layer_indices: vec![0, 2],
            point_indices: vec![vec![1, 3], vec![0]],
            deadline_slot: 10,
        }
    }

    #[test]
    fn validates_structure() {
        let challenge = sample_challenge();
        assert!(challenge.validate().is_ok());
        assert_eq!(challenge.expected_openings(), 3);
        assert!(challenge.contains(0, 1));
        assert!(!challenge.contains(1, 0));
    }

    #[test]
    fn detects_mismatch() {
        let bad = KzgChallenge {
            vcr_id: H256::zero(),
            layer_indices: vec![0],
            point_indices: vec![],
            deadline_slot: 5,
        };
        assert!(bad.validate().is_err());
    }
}
