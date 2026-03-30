use std::collections::HashMap;

/// Feature gate registry for scheduled hard forks.
///
/// Features are activated at specific slots, allowing smooth network
/// upgrades without requiring all nodes to restart simultaneously.
///
/// Usage:
/// ```ignore
/// let mut gates = FeatureGateRegistry::new();
/// gates.schedule("eip1559_fees", 100_000);   // Activate at slot 100K
/// gates.schedule("blob_transactions", 200_000);
///
/// if gates.is_active("eip1559_fees", current_slot) {
///     // Use new fee model
/// }
/// ```
pub struct FeatureGateRegistry {
    /// Feature name → activation slot.
    gates: HashMap<String, u64>,
}

impl FeatureGateRegistry {
    pub fn new() -> Self {
        FeatureGateRegistry {
            gates: HashMap::new(),
        }
    }

    /// Create with default mainnet feature schedule.
    pub fn mainnet() -> Self {
        let mut registry = Self::new();
        // All Phase 0-6 features active from genesis
        registry.schedule("vrf_pos_consensus", 0);
        registry.schedule("hotstuff_bft", 0);
        registry.schedule("bls_aggregation", 0);
        registry.schedule("parallel_execution", 0);
        registry.schedule("kzg_commitments", 0);
        registry.schedule("sparse_merkle_tree", 0);
        registry.schedule("dandelion_privacy", 0);
        registry.schedule("commit_reveal_mev", 0);
        registry.schedule("eip1559_fees", 0);
        registry.schedule("blob_transactions", 0);
        registry.schedule("account_abstraction", 0);
        registry.schedule("conviction_voting", 0);
        registry.schedule("light_client_proofs", 0);
        registry
    }

    /// Schedule a feature to activate at a specific slot.
    pub fn schedule(&mut self, feature: &str, activation_slot: u64) {
        self.gates.insert(feature.to_string(), activation_slot);
    }

    /// Check if a feature is active at the given slot.
    pub fn is_active(&self, feature: &str, current_slot: u64) -> bool {
        self.gates
            .get(feature)
            .map(|&activation| current_slot >= activation)
            .unwrap_or(false)
    }

    /// Get the activation slot for a feature.
    pub fn activation_slot(&self, feature: &str) -> Option<u64> {
        self.gates.get(feature).copied()
    }

    /// List all features and their activation status.
    pub fn list_features(&self, current_slot: u64) -> Vec<(&str, u64, bool)> {
        let mut features: Vec<_> = self
            .gates
            .iter()
            .map(|(name, &slot)| (name.as_str(), slot, current_slot >= slot))
            .collect();
        features.sort_by_key(|(_, slot, _)| *slot);
        features
    }

    /// Count active features at the given slot.
    pub fn active_count(&self, current_slot: u64) -> usize {
        self.gates.values().filter(|&&s| current_slot >= s).count()
    }
}

impl Default for FeatureGateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_not_active_before_slot() {
        let mut gates = FeatureGateRegistry::new();
        gates.schedule("blob_tx", 100_000);

        assert!(!gates.is_active("blob_tx", 99_999));
        assert!(gates.is_active("blob_tx", 100_000));
        assert!(gates.is_active("blob_tx", 200_000));
    }

    #[test]
    fn test_unknown_feature_not_active() {
        let gates = FeatureGateRegistry::new();
        assert!(!gates.is_active("nonexistent", 999_999));
    }

    #[test]
    fn test_mainnet_defaults_active_from_genesis() {
        let gates = FeatureGateRegistry::mainnet();
        assert!(gates.is_active("vrf_pos_consensus", 0));
        assert!(gates.is_active("eip1559_fees", 0));
        assert!(gates.is_active("blob_transactions", 0));
    }

    #[test]
    fn test_list_features() {
        let mut gates = FeatureGateRegistry::new();
        gates.schedule("feature_a", 100);
        gates.schedule("feature_b", 200);

        let list = gates.list_features(150);
        assert_eq!(list.len(), 2);
        // feature_a should be active, feature_b not yet
        let a = list.iter().find(|(n, _, _)| *n == "feature_a").unwrap();
        assert!(a.2); // active
        let b = list.iter().find(|(n, _, _)| *n == "feature_b").unwrap();
        assert!(!b.2); // not yet
    }

    #[test]
    fn test_active_count() {
        let mut gates = FeatureGateRegistry::new();
        gates.schedule("a", 10);
        gates.schedule("b", 20);
        gates.schedule("c", 30);

        assert_eq!(gates.active_count(0), 0);
        assert_eq!(gates.active_count(15), 1);
        assert_eq!(gates.active_count(25), 2);
        assert_eq!(gates.active_count(35), 3);
    }
}
