/// Token emission schedule for SWR staking rewards.
///
/// The emission rate decreases over time:
/// - Year 1: 5% annual inflation
/// - Year 2: 4%
/// - Year 3: 3%
/// - Year 4: 2%
/// - Year 5+: 1% (terminal rate)
///
/// Rewards are distributed per-epoch to validators proportional to their stake.
///
/// Fee distribution (from EIP-1559 fee market):
/// - Base fee × gas_used: BURNED (removed from supply)
/// - 60% of priority fee (tip): to block proposer
/// - 40% of priority fee (tip): to treasury

/// Slots per year (500ms slots × 2 per second × 86400 seconds/day × 365 days).
pub const SLOTS_PER_YEAR: u64 = 63_072_000;

/// Slots per epoch (6 hours = 43,200 slots at 500ms).
pub const SLOTS_PER_EPOCH: u64 = 43_200;

/// Epochs per year.
pub const EPOCHS_PER_YEAR: u64 = SLOTS_PER_YEAR / SLOTS_PER_EPOCH; // ~1,460

/// Emission schedule: calculates per-epoch rewards.
#[derive(Debug, Clone)]
pub struct EmissionSchedule {
    /// Initial total supply of SWR.
    pub initial_supply: u128,
    /// Genesis slot (when the chain started).
    pub genesis_slot: u64,
}

impl EmissionSchedule {
    pub fn new(initial_supply: u128) -> Self {
        EmissionSchedule {
            initial_supply,
            genesis_slot: 0,
        }
    }

    /// Get the annual inflation rate (basis points) for a given year.
    ///
    /// Year 0 (first year): 500 bps (5%)
    /// Year 1: 400 bps (4%)
    /// Year 2: 300 bps (3%)
    /// Year 3: 200 bps (2%)
    /// Year 4+: 100 bps (1%) — terminal rate
    pub fn annual_rate_bps(&self, year: u64) -> u64 {
        match year {
            0 => 500,
            1 => 400,
            2 => 300,
            3 => 200,
            _ => 100, // Terminal rate: 1%
        }
    }

    /// Which year does this slot fall in?
    pub fn year_for_slot(&self, slot: u64) -> u64 {
        slot.saturating_sub(self.genesis_slot) / SLOTS_PER_YEAR
    }

    /// Calculate total emission for one epoch.
    ///
    /// epoch_emission = (annual_rate / 10000) * total_supply / epochs_per_year
    pub fn epoch_emission(&self, current_slot: u64, total_supply: u128) -> u128 {
        let year = self.year_for_slot(current_slot);
        let rate_bps = self.annual_rate_bps(year) as u128;

        // annual_emission = total_supply * rate_bps / 10_000
        // epoch_emission = annual_emission / EPOCHS_PER_YEAR
        total_supply
            .saturating_mul(rate_bps)
            / (10_000 * EPOCHS_PER_YEAR as u128)
    }

    /// Fee distribution: split priority fees between proposer and treasury.
    pub fn distribute_priority_fee(priority_fee: u128) -> (u128, u128) {
        let proposer_share = (priority_fee * 60) / 100; // 60% to proposer
        let treasury_share = priority_fee - proposer_share; // 40% to treasury
        (proposer_share, treasury_share)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annual_rate_schedule() {
        let schedule = EmissionSchedule::new(1_000_000_000);
        assert_eq!(schedule.annual_rate_bps(0), 500);
        assert_eq!(schedule.annual_rate_bps(1), 400);
        assert_eq!(schedule.annual_rate_bps(2), 300);
        assert_eq!(schedule.annual_rate_bps(3), 200);
        assert_eq!(schedule.annual_rate_bps(4), 100);
        assert_eq!(schedule.annual_rate_bps(100), 100); // Terminal
    }

    #[test]
    fn test_year_for_slot() {
        let schedule = EmissionSchedule::new(1_000_000_000);
        assert_eq!(schedule.year_for_slot(0), 0);
        assert_eq!(schedule.year_for_slot(SLOTS_PER_YEAR - 1), 0);
        assert_eq!(schedule.year_for_slot(SLOTS_PER_YEAR), 1);
        assert_eq!(schedule.year_for_slot(SLOTS_PER_YEAR * 5), 5);
    }

    #[test]
    fn test_epoch_emission_year_1() {
        let schedule = EmissionSchedule::new(1_000_000_000);
        let supply: u128 = 1_000_000_000_000_000; // 1B tokens with 6 decimals

        let emission = schedule.epoch_emission(0, supply);

        // 5% annual / 1460 epochs ≈ 0.00342% per epoch
        // 1_000_000_000_000_000 * 500 / 10_000 / 1460 = 34_246_575_342
        assert!(emission > 0);
        let annual = emission * EPOCHS_PER_YEAR as u128;
        let annual_rate = (annual * 10_000) / supply;
        assert!(
            annual_rate >= 490 && annual_rate <= 510,
            "annual rate should be ~5%, got {} bps",
            annual_rate
        );
    }

    #[test]
    fn test_epoch_emission_decreases_over_years() {
        let schedule = EmissionSchedule::new(1_000_000_000);
        let supply: u128 = 1_000_000_000_000_000;

        let e0 = schedule.epoch_emission(0, supply);
        let e1 = schedule.epoch_emission(SLOTS_PER_YEAR, supply);
        let e4 = schedule.epoch_emission(SLOTS_PER_YEAR * 4, supply);

        assert!(e0 > e1, "year 0 emission > year 1");
        assert!(e1 > e4, "year 1 emission > year 4");
    }

    #[test]
    fn test_priority_fee_distribution() {
        let (proposer, treasury) = EmissionSchedule::distribute_priority_fee(1_000_000);
        assert_eq!(proposer, 600_000); // 60%
        assert_eq!(treasury, 400_000); // 40%
    }
}
