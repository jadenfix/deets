use std::cmp::Ordering;

use aether_types::{Address, Slot, H256};

use crate::scoring::{HardwareTier, ProviderReputation};

pub fn top_providers<'a>(
    providers: &'a [ProviderReputation],
    model: H256,
    minimum_score: f64,
    tier: HardwareTier,
    current_slot: Slot,
    staleness_threshold: Slot,
    limit: usize,
) -> Vec<&'a ProviderReputation> {
    let mut candidates: Vec<&ProviderReputation> = providers
        .iter()
        .filter(|provider| provider.score >= minimum_score)
        .filter(|provider| provider.hardware_tier >= tier)
        .filter(|provider| provider.supported_models.contains(&model))
        .filter(|provider| current_slot - provider.last_active_slot <= staleness_threshold)
        .collect();

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
    candidates.truncate(limit);
    candidates
}

pub fn provider_addresses(providers: &[&ProviderReputation]) -> Vec<Address> {
    providers.iter().map(|provider| provider.address).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::HardwareTier;

    #[test]
    fn selects_top_providers() {
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();
        let mut p1 = ProviderReputation::new(addr1, HardwareTier::Standard);
        let mut p2 = ProviderReputation::new(addr2, HardwareTier::Premium);
        let model = H256::zero();
        p1.add_model(model);
        p2.add_model(model);
        p1.score = 60.0;
        p1.last_active_slot = 10;
        p2.score = 80.0;
        p2.last_active_slot = 10;
        let providers = vec![p1, p2];

        let selected = top_providers(&providers, model, 50.0, HardwareTier::Standard, 12, 10, 2);
        assert_eq!(selected.len(), 2);
        let addresses = provider_addresses(&selected);
        assert_eq!(addresses[0], addr2);
    }
}
