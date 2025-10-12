use std::collections::HashSet;

use aether_crypto_kzg::KzgVerifier;

use crate::challenge::KzgChallenge;
use crate::error::{Result, VerifierError};
use crate::opening::{KzgOpeningResponse, Opening};

pub fn verify_kzg_openings(
    verifier: &KzgVerifier,
    challenge: &KzgChallenge,
    response: &KzgOpeningResponse,
) -> Result<()> {
    challenge.validate()?;

    if challenge.vcr_id != response.vcr_id {
        return Err(VerifierError::VcrMismatch {
            expected: challenge.vcr_id,
            received: response.vcr_id,
        });
    }

    let expected = challenge.expected_openings();
    if expected != response.openings.len() {
        return Err(VerifierError::IncompleteResponse {
            expected,
            received: response.openings.len(),
        });
    }

    let mut seen = HashSet::new();

    for opening in &response.openings {
        validate_opening(challenge, opening, &mut seen)?;
        verifier
            .verify(&opening.commitment, &opening.proof, &opening.point)
            .map_err(|err| VerifierError::InvalidProof {
                layer: opening.layer_idx,
                point: opening.point_idx,
                source: err,
            })?;
    }

    Ok(())
}

fn validate_opening(
    challenge: &KzgChallenge,
    opening: &Opening,
    seen: &mut HashSet<(u32, u32)>,
) -> Result<()> {
    if !challenge.contains(opening.layer_idx, opening.point_idx) {
        return Err(VerifierError::UnexpectedOpening {
            layer: opening.layer_idx,
            point: opening.point_idx,
        });
    }

    if !seen.insert((opening.layer_idx, opening.point_idx)) {
        return Err(VerifierError::DuplicateOpening {
            layer: opening.layer_idx,
            point: opening.point_idx,
        });
    }

    if opening.point.len() != 32 {
        return Err(VerifierError::InvalidChallenge(
            "evaluation point must be 32 bytes",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_kzg::{KzgCommitment, KzgProof};
    use aether_types::H256;

    fn sample_challenge() -> KzgChallenge {
        KzgChallenge {
            vcr_id: H256::zero(),
            layer_indices: vec![0],
            point_indices: vec![vec![0, 1]],
            deadline_slot: 50,
        }
    }

    fn sample_openings() -> Vec<Opening> {
        vec![
            Opening {
                layer_idx: 0,
                point_idx: 0,
                point: vec![1u8; 32],
                commitment: KzgCommitment {
                    commitment: vec![1u8; 48],
                },
                proof: KzgProof {
                    proof: vec![2u8; 48],
                    evaluation: vec![3u8; 32],
                },
            },
            Opening {
                layer_idx: 0,
                point_idx: 1,
                point: vec![4u8; 32],
                commitment: KzgCommitment {
                    commitment: vec![1u8; 48],
                },
                proof: KzgProof {
                    proof: vec![2u8; 48],
                    evaluation: vec![3u8; 32],
                },
            },
        ]
    }

    #[test]
    fn verifies_valid_openings() {
        let verifier = KzgVerifier::new(1024);
        let challenge = sample_challenge();
        let openings = sample_openings();
        let response = KzgOpeningResponse::new(H256::zero(), openings);

        assert!(verify_kzg_openings(&verifier, &challenge, &response).is_ok());
    }

    #[test]
    fn detects_mismatch() {
        let verifier = KzgVerifier::new(1024);
        let mut challenge = sample_challenge();
        let openings = sample_openings();
        let response = KzgOpeningResponse::new(H256::zero(), openings);

        assert!(verify_kzg_openings(&verifier, &challenge, &response).is_err());
        challenge.point_indices[0].push(2);
        let response = KzgOpeningResponse::new(H256::zero(), vec![]);
        assert!(verify_kzg_openings(&verifier, &challenge, &response).is_err());
    }
}
