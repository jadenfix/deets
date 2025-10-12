use rand::{rngs::StdRng, seq::IteratorRandom, SeedableRng};

use crate::challenge::KzgChallenge;
use aether_types::H256;

/// Build a random challenge selecting a subset of layers and evaluation points.
///
/// The function is deterministic when a seed is provided which keeps tests stable.
pub fn build_challenge(
    vcr_id: H256,
    total_layers: u32,
    layer_size: u32,
    sample_layers: usize,
    sample_points: usize,
    seed: Option<u64>,
) -> KzgChallenge {
    assert!(total_layers > 0, "total layers must be positive");
    assert!(layer_size > 0, "layer size must be positive");
    assert!(sample_layers > 0, "must request at least one layer");
    assert!(
        sample_points > 0,
        "must request at least one point per layer"
    );

    let mut rng = seed
        .map(StdRng::seed_from_u64)
        .unwrap_or_else(|| StdRng::from_rng(rand::thread_rng()).expect("rng"));

    let mut layers: Vec<u32> = (0..total_layers).collect();
    layers.sort();

    let selected_layers: Vec<u32> = layers
        .into_iter()
        .choose_multiple(&mut rng, sample_layers.min(total_layers as usize));

    let mut point_indices = Vec::with_capacity(selected_layers.len());
    for _layer in &selected_layers {
        let indices: Vec<u32> = (0..layer_size).collect();
        let chosen = indices
            .into_iter()
            .choose_multiple(&mut rng, sample_points.min(layer_size as usize));
        point_indices.push(chosen);
    }

    KzgChallenge {
        vcr_id,
        layer_indices: selected_layers,
        point_indices,
        deadline_slot: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_with_seed() {
        let seed = Some(42);
        let challenge1 = build_challenge(H256::zero(), 8, 16, 3, 4, seed);
        let challenge2 = build_challenge(H256::zero(), 8, 16, 3, 4, seed);
        assert_eq!(challenge1.layer_indices, challenge2.layer_indices);
        assert_eq!(challenge1.point_indices, challenge2.point_indices);
    }
}
