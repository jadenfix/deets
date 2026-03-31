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
        let point: [u8; 32] = opening
            .point
            .as_slice()
            .try_into()
            .map_err(|_| VerifierError::InvalidChallenge("point must be 32 bytes"))?;
        let valid = verifier
            .verify(&opening.commitment, &opening.proof, &point)
            .map_err(|err| VerifierError::InvalidProof {
                layer: opening.layer_idx,
                point: opening.point_idx,
                source: err,
            })?;
        if !valid {
            return Err(VerifierError::InvalidProof {
                layer: opening.layer_idx,
                point: opening.point_idx,
                source: anyhow::anyhow!("KZG proof verification failed"),
            });
        }
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
    use aether_crypto_kzg::{KzgCommitment, KzgProof, ScalarBytes};
    use aether_types::H256;

    fn sample_challenge() -> KzgChallenge {
        KzgChallenge {
            vcr_id: H256::zero(),
            layer_indices: vec![0],
            point_indices: vec![vec![0, 1]],
            deadline_slot: 50,
        }
    }

    fn make_valid_openings(verifier: &KzgVerifier) -> Vec<Opening> {
        // Create a real polynomial and generate valid commitments/proofs
        let mut coeffs = vec![[0u8; 32]; 3];
        coeffs[0][0] = 3;
        coeffs[1][0] = 2;
        coeffs[2][0] = 1;

        let commitment = verifier.commit(&coeffs).unwrap();

        let mut z1 = [0u8; 32];
        z1[0] = 1;
        let mut z2 = [0u8; 32];
        z2[0] = 4;

        let proof1 = verifier.create_proof(&coeffs, &z1).unwrap();
        let proof2 = verifier.create_proof(&coeffs, &z2).unwrap();

        vec![
            Opening {
                layer_idx: 0,
                point_idx: 0,
                point: z1.to_vec(),
                commitment: commitment.clone(),
                proof: proof1,
            },
            Opening {
                layer_idx: 0,
                point_idx: 1,
                point: z2.to_vec(),
                commitment,
                proof: proof2,
            },
        ]
    }

    #[test]
    fn verifies_valid_openings() {
        let verifier = KzgVerifier::new_insecure_test(1024);
        let challenge = sample_challenge();
        let openings = make_valid_openings(&verifier);
        let response = KzgOpeningResponse::new(H256::zero(), openings);

        assert!(verify_kzg_openings(&verifier, &challenge, &response).is_ok());
    }

    #[test]
    fn detects_mismatch() {
        let verifier = KzgVerifier::new_insecure_test(1024);
        let mut challenge = sample_challenge();
        let openings = make_valid_openings(&verifier);
        let response =
            KzgOpeningResponse::new(H256::from_slice(&[9u8; 32]).unwrap(), openings);

        assert!(verify_kzg_openings(&verifier, &challenge, &response).is_err());
        challenge.point_indices[0].push(2);
        let response = KzgOpeningResponse::new(H256::zero(), vec![]);
        assert!(verify_kzg_openings(&verifier, &challenge, &response).is_err());
    }
}
