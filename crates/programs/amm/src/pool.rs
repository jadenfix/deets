use aether_types::{Address, H256};
use num_bigint::BigUint;
use num_traits::{One, ToPrimitive};
use serde::{Deserialize, Serialize};

/// Constant Product AMM (x * y = k)
///
/// Features:
/// - Token swaps (A <-> B)
/// - Liquidity provision (add/remove)
/// - LP tokens
/// - Fee collection (0.3%)
/// - Slippage protection
///
/// Formula:
/// - Invariant: x * y = k
/// - Swap output: dy = (dx * fee * y) / (x + dx * fee)
/// - Where fee = 0.997 (0.3% fee)
///
/// Fixed-point arithmetic:
/// - Use Q64.64 for precision
/// - All amounts in smallest unit

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidityPool {
    pub pool_id: H256,
    pub token_a: Address,
    pub token_b: Address,
    pub reserve_a: u128,
    pub reserve_b: u128,
    pub lp_token_supply: u128,
    pub fee_bps: u32, // Basis points (30 = 0.3%)
}

impl LiquidityPool {
    pub fn new(
        pool_id: H256,
        token_a: Address,
        token_b: Address,
        fee_bps: u32,
    ) -> Result<Self, String> {
        if fee_bps > 10000 {
            return Err("fee_bps must be <= 10000".to_string());
        }
        Ok(LiquidityPool {
            pool_id,
            token_a,
            token_b,
            reserve_a: 0,
            reserve_b: 0,
            lp_token_supply: 0,
            fee_bps,
        })
    }

    /// Add liquidity to the pool
    pub fn add_liquidity(
        &mut self,
        amount_a: u128,
        amount_b: u128,
        min_lp_tokens: u128,
    ) -> Result<u128, String> {
        if amount_a == 0 || amount_b == 0 {
            return Err("amounts must be non-zero".to_string());
        }

        let lp_tokens = if self.lp_token_supply == 0 {
            // Initial liquidity mints sqrt(amount_a * amount_b).
            let product = BigUint::from(amount_a) * BigUint::from(amount_b);
            let liquidity = integer_sqrt_biguint(&product)
                .to_u128()
                .ok_or("overflow in initial liquidity")?;

            if liquidity < 1000 {
                return Err("insufficient initial liquidity".to_string());
            }
            if liquidity < min_lp_tokens {
                return Err("insufficient LP tokens".to_string());
            }

            self.reserve_a = amount_a;
            self.reserve_b = amount_b;
            self.lp_token_supply = liquidity;

            liquidity
        } else {
            let ratio_lhs = amount_a
                .checked_mul(self.reserve_b)
                .ok_or("overflow in liquidity ratio check")?;
            let ratio_rhs = amount_b
                .checked_mul(self.reserve_a)
                .ok_or("overflow in liquidity ratio check")?;
            if ratio_lhs != ratio_rhs {
                return Err("liquidity must be added at the current pool ratio".to_string());
            }

            // Proportional liquidity — multiply before dividing for precision
            let liquidity_a = mul_div(amount_a, self.lp_token_supply, self.reserve_a)?;
            let liquidity_b = mul_div(amount_b, self.lp_token_supply, self.reserve_b)?;

            let liquidity = liquidity_a.min(liquidity_b);

            if liquidity == 0 || liquidity < min_lp_tokens {
                return Err("insufficient LP tokens".to_string());
            }

            self.reserve_a = self
                .reserve_a
                .checked_add(amount_a)
                .ok_or("reserve_a overflow")?;
            self.reserve_b = self
                .reserve_b
                .checked_add(amount_b)
                .ok_or("reserve_b overflow")?;
            self.lp_token_supply = self
                .lp_token_supply
                .checked_add(liquidity)
                .ok_or("lp_token_supply overflow")?;

            liquidity
        };

        Ok(lp_tokens)
    }

    /// Remove liquidity from the pool
    pub fn remove_liquidity(
        &mut self,
        lp_tokens: u128,
        min_amount_a: u128,
        min_amount_b: u128,
    ) -> Result<(u128, u128), String> {
        if lp_tokens == 0 {
            return Err("amount must be non-zero".to_string());
        }

        if lp_tokens > self.lp_token_supply {
            return Err("insufficient LP tokens".to_string());
        }

        let amount_a = mul_div(lp_tokens, self.reserve_a, self.lp_token_supply)
            .map_err(|e| format!("remove_liquidity amount_a: {}", e))?;
        let amount_b = mul_div(lp_tokens, self.reserve_b, self.lp_token_supply)
            .map_err(|e| format!("remove_liquidity amount_b: {}", e))?;

        if amount_a < min_amount_a || amount_b < min_amount_b {
            return Err("insufficient output amount".to_string());
        }

        self.reserve_a = self
            .reserve_a
            .checked_sub(amount_a)
            .ok_or("reserve_a underflow")?;
        self.reserve_b = self
            .reserve_b
            .checked_sub(amount_b)
            .ok_or("reserve_b underflow")?;
        self.lp_token_supply = self
            .lp_token_supply
            .checked_sub(lp_tokens)
            .ok_or("lp_token_supply underflow")?;

        Ok((amount_a, amount_b))
    }

    /// Swap token A for token B
    pub fn swap_a_to_b(&mut self, amount_in: u128, min_amount_out: u128) -> Result<u128, String> {
        if amount_in == 0 {
            return Err("amount must be non-zero".to_string());
        }

        let k_old = BigUint::from(self.reserve_a) * BigUint::from(self.reserve_b);

        let amount_out = self.get_amount_out(amount_in, self.reserve_a, self.reserve_b)?;

        if amount_out < min_amount_out {
            return Err("insufficient output amount".to_string());
        }

        self.reserve_a = self
            .reserve_a
            .checked_add(amount_in)
            .ok_or("reserve_a overflow")?;
        self.reserve_b = self
            .reserve_b
            .checked_sub(amount_out)
            .ok_or("reserve_b underflow")?;

        // Verify invariant: k must not decrease
        self.check_invariant_big(&k_old)?;

        Ok(amount_out)
    }

    /// Swap token B for token A
    pub fn swap_b_to_a(&mut self, amount_in: u128, min_amount_out: u128) -> Result<u128, String> {
        if amount_in == 0 {
            return Err("amount must be non-zero".to_string());
        }

        let k_old = BigUint::from(self.reserve_b) * BigUint::from(self.reserve_a);

        let amount_out = self.get_amount_out(amount_in, self.reserve_b, self.reserve_a)?;

        if amount_out < min_amount_out {
            return Err("insufficient output amount".to_string());
        }

        self.reserve_b = self
            .reserve_b
            .checked_add(amount_in)
            .ok_or("reserve_b overflow")?;
        self.reserve_a = self
            .reserve_a
            .checked_sub(amount_out)
            .ok_or("reserve_a underflow")?;

        // Verify invariant: k must not decrease
        self.check_invariant_big(&k_old)?;

        Ok(amount_out)
    }

    /// Calculate output amount for a swap
    /// Formula: amount_out = (amount_in * fee * reserve_out) / (reserve_in * 10000 + amount_in * fee)
    ///
    /// Uses BigUint to avoid overflow when reserves or amounts are large.
    fn get_amount_out(
        &self,
        amount_in: u128,
        reserve_in: u128,
        reserve_out: u128,
    ) -> Result<u128, String> {
        if amount_in == 0 || reserve_in == 0 || reserve_out == 0 {
            return Err("invalid reserves".to_string());
        }

        let fee_multiplier = 10000u128 - self.fee_bps as u128;
        let amount_in_with_fee = BigUint::from(amount_in) * BigUint::from(fee_multiplier);

        let numerator = &amount_in_with_fee * BigUint::from(reserve_out);
        let denominator =
            BigUint::from(reserve_in) * BigUint::from(10000u128) + &amount_in_with_fee;

        let amount_out = (&numerator / &denominator)
            .to_u128()
            .ok_or("swap output overflow")?;

        Ok(amount_out)
    }

    /// Check constant product invariant using BigUint: k_new must be >= k_old
    fn check_invariant_big(&self, k_old: &BigUint) -> Result<(), String> {
        let k_new = BigUint::from(self.reserve_a) * BigUint::from(self.reserve_b);

        if k_new == BigUint::ZERO {
            return Err("invariant violated: k = 0".to_string());
        }

        if k_new < *k_old {
            return Err("invariant violated: k decreased".to_string());
        }

        Ok(())
    }

    /// Get current price (reserve_b / reserve_a)
    pub fn get_price(&self) -> Result<u128, String> {
        if self.reserve_a == 0 {
            return Err("zero reserve".to_string());
        }

        self.reserve_b
            .checked_mul(1_000_000)
            .ok_or_else(|| "overflow in price calculation".to_string())
            .map(|n| n / self.reserve_a)
    }
}

/// Safe multiplication then division: computes a * b / c without intermediate overflow
/// when possible, falling back to an error if the product overflows u128.
fn mul_div(a: u128, b: u128, c: u128) -> Result<u128, String> {
    if c == 0 {
        return Err("division by zero in proportional calculation".to_string());
    }
    // Try direct multiplication first
    if let Some(ab) = a.checked_mul(b) {
        return Ok(ab / c);
    }
    // Overflow: use wider arithmetic via (a/c)*b + (a%c)*b/c
    let whole = (a / c)
        .checked_mul(b)
        .ok_or("overflow in proportional calculation")?;
    let remainder = (a % c)
        .checked_mul(b)
        .ok_or("overflow in proportional calculation")?
        / c;
    whole
        .checked_add(remainder)
        .ok_or_else(|| "overflow in proportional calculation".to_string())
}

fn integer_sqrt_biguint(value: &BigUint) -> BigUint {
    if value < &BigUint::from(2u8) {
        return value.clone();
    }

    let two = BigUint::from(2u8);
    let mut x = value.clone();
    let mut y = (&x + BigUint::one()) / &two;

    while y < x {
        x = y.clone();
        y = (&x + value / &x) / &two;
    }

    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool() -> LiquidityPool {
        LiquidityPool::new(
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            Address::from_slice(&[2u8; 20]).unwrap(),
            30, // 0.3% fee
        )
        .unwrap()
    }

    #[test]
    fn test_add_initial_liquidity() {
        let mut pool = test_pool();

        let lp_tokens = pool.add_liquidity(10000, 10000, 0).unwrap();

        assert_eq!(pool.reserve_a, 10000);
        assert_eq!(pool.reserve_b, 10000);
        // sqrt(10000) * sqrt(10000) = 100 * 100 = 10000
        assert_eq!(lp_tokens, 10000);
    }

    #[test]
    fn test_add_initial_liquidity_uses_exact_product_sqrt() {
        let mut pool = test_pool();

        let lp_tokens = pool.add_liquidity(2_000, 8_000, 0).unwrap();

        assert_eq!(lp_tokens, 4_000);
    }

    #[test]
    fn test_add_initial_liquidity_handles_large_balanced_values() {
        let mut pool = test_pool();
        let amount = 1u128 << 80;

        let lp_tokens = pool.add_liquidity(amount, amount, 0).unwrap();

        assert_eq!(lp_tokens, amount);
    }

    #[test]
    fn test_add_initial_liquidity_respects_min_lp_tokens() {
        let mut pool = test_pool();

        let result = pool.add_liquidity(10_000, 10_000, 10_001);
        assert!(result.is_err());
        assert_eq!(pool.reserve_a, 0);
        assert_eq!(pool.reserve_b, 0);
        assert_eq!(pool.lp_token_supply, 0);
    }

    #[test]
    fn test_add_proportional_liquidity() {
        let mut pool = test_pool();

        pool.add_liquidity(1000, 2000, 0).unwrap();
        let lp_tokens = pool.add_liquidity(500, 1000, 0).unwrap();

        assert_eq!(pool.reserve_a, 1500);
        assert_eq!(pool.reserve_b, 3000);
        assert!(lp_tokens > 0);
    }

    #[test]
    fn test_add_non_proportional_liquidity_rejected() {
        let mut pool = test_pool();

        pool.add_liquidity(1_000, 2_000, 0).unwrap();
        let result = pool.add_liquidity(500, 900, 0);

        assert!(result.is_err());
        assert_eq!(pool.reserve_a, 1_000);
        assert_eq!(pool.reserve_b, 2_000);
        assert_eq!(pool.lp_token_supply, 1_414);
    }

    #[test]
    fn test_remove_liquidity() {
        let mut pool = test_pool();

        let lp_tokens = pool.add_liquidity(1000, 2000, 0).unwrap();
        let (amount_a, amount_b) = pool.remove_liquidity(lp_tokens / 2, 0, 0).unwrap();

        assert!(amount_a > 0);
        assert!(amount_b > 0);
        assert_eq!(amount_a * 2, amount_b); // Proportional
    }

    #[test]
    fn test_swap() {
        let mut pool = test_pool();

        pool.add_liquidity(1000, 2000, 0).unwrap();

        // Swap 100 of token A for token B
        let amount_out = pool.swap_a_to_b(100, 0).unwrap();

        assert!(amount_out > 0);
        assert!(amount_out < 200); // Less than proportional due to slippage
    }

    #[test]
    fn test_constant_product() {
        let mut pool = test_pool();

        pool.add_liquidity(10000, 10000, 0).unwrap();
        let k_before = pool.reserve_a * pool.reserve_b;

        pool.swap_a_to_b(100, 0).unwrap();
        let k_after = pool.reserve_a * pool.reserve_b;

        // k should increase (due to fees)
        assert!(k_after >= k_before);
    }

    #[test]
    fn test_price() {
        let mut pool = test_pool();

        pool.add_liquidity(1000, 2000, 0).unwrap();
        let price = pool.get_price().unwrap();

        // Price should be 2:1 (2_000_000)
        assert_eq!(price, 2_000_000);
    }

    // ── Adversarial tests ────────────────────────────────────

    #[test]
    fn test_invariant_cannot_decrease_after_swap() {
        let mut pool = test_pool();
        pool.add_liquidity(100_000, 100_000, 0).unwrap();

        let k_before = pool.reserve_a * pool.reserve_b;
        pool.swap_a_to_b(5_000, 0).unwrap();
        let k_after = pool.reserve_a * pool.reserve_b;

        assert!(
            k_after >= k_before,
            "invariant decreased: k_before={k_before}, k_after={k_after}"
        );
    }

    #[test]
    fn test_swap_with_zero_amount_rejected() {
        let mut pool = test_pool();
        pool.add_liquidity(10_000, 10_000, 0).unwrap();

        let result = pool.swap_a_to_b(0, 0);
        assert!(result.is_err(), "swap of 0 tokens should be rejected");
    }

    // ── swap_b_to_a tests ───────────────────────────────────

    #[test]
    fn test_swap_b_to_a_happy_path() {
        let mut pool = test_pool();
        pool.add_liquidity(1000, 2000, 0).unwrap();

        let amount_out = pool.swap_b_to_a(200, 0).unwrap();

        assert!(amount_out > 0);
        assert!(
            amount_out < 100,
            "should be less than proportional due to slippage"
        );
        assert_eq!(pool.reserve_b, 2200);
        assert_eq!(pool.reserve_a, 1000 - amount_out);
    }

    #[test]
    fn test_swap_b_to_a_constant_product() {
        let mut pool = test_pool();
        pool.add_liquidity(10000, 10000, 0).unwrap();
        let k_before = pool.reserve_a * pool.reserve_b;

        pool.swap_b_to_a(100, 0).unwrap();
        let k_after = pool.reserve_a * pool.reserve_b;

        assert!(k_after >= k_before, "invariant must not decrease");
    }

    #[test]
    fn test_swap_b_to_a_min_amount_out_enforced() {
        let mut pool = test_pool();
        pool.add_liquidity(10000, 10000, 0).unwrap();

        let result = pool.swap_b_to_a(100, u128::MAX);
        assert!(result.is_err());
    }

    #[test]
    fn test_swap_b_to_a_zero_rejected() {
        let mut pool = test_pool();
        pool.add_liquidity(10000, 10000, 0).unwrap();

        assert!(pool.swap_b_to_a(0, 0).is_err());
    }

    #[test]
    fn test_swap_b_to_a_symmetry() {
        // Two identical pools, swap same amount in opposite directions
        let mut pool_ab = test_pool();
        let mut pool_ba = test_pool();
        pool_ab.add_liquidity(10000, 10000, 0).unwrap();
        pool_ba.add_liquidity(10000, 10000, 0).unwrap();

        let out_ab = pool_ab.swap_a_to_b(500, 0).unwrap();
        let out_ba = pool_ba.swap_b_to_a(500, 0).unwrap();

        // With equal reserves, A→B and B→A should yield identical output
        assert_eq!(out_ab, out_ba);
    }

    // ── get_price overflow test ─────────────────────────────

    #[test]
    fn test_get_price_overflow_returns_error() {
        let mut pool = test_pool();
        // reserve_b near u128::MAX / 1_000_000 will not overflow
        // but above that threshold it should error, not silently saturate
        pool.reserve_a = 1;
        pool.reserve_b = u128::MAX;
        pool.lp_token_supply = 1;

        let result = pool.get_price();
        assert!(
            result.is_err(),
            "get_price must error on overflow, not saturate"
        );
    }

    #[test]
    fn test_get_price_large_but_valid() {
        let mut pool = test_pool();
        pool.reserve_a = 1_000_000;
        pool.reserve_b = u128::MAX / 1_000_000; // just below overflow threshold
        pool.lp_token_supply = 1;

        let price = pool.get_price().unwrap();
        assert!(price > 0);
    }

    #[test]
    fn test_fee_bps_above_10000_rejected() {
        let result = LiquidityPool::new(
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            Address::from_slice(&[2u8; 20]).unwrap(),
            10001,
        );
        assert!(result.is_err(), "fee_bps > 10000 must be rejected");
    }

    #[test]
    fn test_swap_large_reserves_no_overflow() {
        // With u128 checked_mul, reserves above ~u64::MAX would overflow and
        // reject swaps.  BigUint arithmetic handles this correctly.
        let mut pool = test_pool();
        let big = 1u128 << 100; // ~1.27e30
        pool.reserve_a = big;
        pool.reserve_b = big;
        pool.lp_token_supply = big;

        let amount_in = 1u128 << 80;
        let out = pool.swap_a_to_b(amount_in, 0).unwrap();
        assert!(out > 0);
        assert!(
            out < amount_in,
            "output must be less than input due to fees and slippage"
        );
        // Invariant: reserves still positive
        assert!(pool.reserve_a > big);
        assert!(pool.reserve_b < big);
    }

    #[test]
    fn test_swap_b_to_a_large_reserves() {
        let mut pool = test_pool();
        let big = 1u128 << 100;
        pool.reserve_a = big;
        pool.reserve_b = big;
        pool.lp_token_supply = big;

        let amount_in = 1u128 << 80;
        let out = pool.swap_b_to_a(amount_in, 0).unwrap();
        assert!(out > 0);
        assert!(out < amount_in);
    }

    #[test]
    fn test_fee_bps_at_boundary_accepted() {
        let pool = LiquidityPool::new(
            H256::zero(),
            Address::from_slice(&[1u8; 20]).unwrap(),
            Address::from_slice(&[2u8; 20]).unwrap(),
            10000, // 100% fee — extreme but valid
        );
        assert!(pool.is_ok());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use num_bigint::BigUint;
    use proptest::prelude::*;

    /// Build a pool with some initial liquidity already added.
    /// `reserve_a` and `reserve_b` are set directly to avoid the ratio constraint
    /// on secondary liquidity additions.  `lp_token_supply` is set to the geometric
    /// mean so the proportions remain consistent.
    fn seeded_pool(ra: u128, rb: u128, fee_bps: u32) -> LiquidityPool {
        let lp = integer_sqrt_biguint(&(BigUint::from(ra) * BigUint::from(rb)))
            .to_u128()
            .unwrap_or(1)
            .max(1);
        LiquidityPool {
            pool_id: H256::zero(),
            token_a: Address::from_slice(&[1u8; 20]).unwrap(),
            token_b: Address::from_slice(&[2u8; 20]).unwrap(),
            reserve_a: ra,
            reserve_b: rb,
            lp_token_supply: lp,
            fee_bps,
        }
    }

    /// Reserve sizes: use values up to 2^64 to avoid BigUint overflow in k checks.
    fn arb_reserve() -> impl Strategy<Value = u128> {
        // Non-zero, up to ~1e19 (just above u64::MAX) so the product fits in a
        // reasonable BigUint without hitting u128-return overflow in get_amount_out.
        1u128..=1_000_000_000_000_000_000u128
    }

    proptest! {
        /// Constant product invariant: k must not decrease after a valid A→B swap.
        #[test]
        fn invariant_holds_after_swap_a_to_b(
            ra in arb_reserve(),
            rb in arb_reserve(),
            fee_bps in 0u32..=300,
        ) {
            let mut pool = seeded_pool(ra, rb, fee_bps);
            let k_before = BigUint::from(pool.reserve_a) * BigUint::from(pool.reserve_b);

            // Swap at most 10% of reserve_a so the swap succeeds
            let amount_in = (ra / 10).max(1);
            if let Ok(_out) = pool.swap_a_to_b(amount_in, 0) {
                let k_after = BigUint::from(pool.reserve_a) * BigUint::from(pool.reserve_b);
                prop_assert!(k_after >= k_before,
                    "k decreased: k_before={k_before}, k_after={k_after}");
            }
        }

        /// Constant product invariant: k must not decrease after a valid B→A swap.
        #[test]
        fn invariant_holds_after_swap_b_to_a(
            ra in arb_reserve(),
            rb in arb_reserve(),
            fee_bps in 0u32..=300,
        ) {
            let mut pool = seeded_pool(ra, rb, fee_bps);
            let k_before = BigUint::from(pool.reserve_a) * BigUint::from(pool.reserve_b);

            let amount_in = (rb / 10).max(1);
            if let Ok(_out) = pool.swap_b_to_a(amount_in, 0) {
                let k_after = BigUint::from(pool.reserve_a) * BigUint::from(pool.reserve_b);
                prop_assert!(k_after >= k_before,
                    "k decreased: k_before={k_before}, k_after={k_after}");
            }
        }

        /// Swap output is always strictly less than the output reserve.
        #[test]
        fn swap_output_less_than_reserve(
            ra in arb_reserve(),
            rb in arb_reserve(),
        ) {
            let mut pool = seeded_pool(ra, rb, 30);
            let amount_in = (ra / 10).max(1);
            if let Ok(out) = pool.swap_a_to_b(amount_in, 0) {
                prop_assert!(out < rb,
                    "swap drained the pool: out={out} >= reserve_b={rb}");
            }
        }

        /// Reserves stay positive after any swap (pool never fully drained).
        #[test]
        fn reserves_remain_positive_after_swap(
            ra in arb_reserve(),
            rb in arb_reserve(),
            amount_in in 1u128..=100_000_000u128,
        ) {
            let mut pool = seeded_pool(ra, rb, 30);
            if pool.swap_a_to_b(amount_in, 0).is_ok() {
                prop_assert!(pool.reserve_a > 0);
                prop_assert!(pool.reserve_b > 0);
            }
            // Reset and test B→A direction
            let mut pool2 = seeded_pool(ra, rb, 30);
            if pool2.swap_b_to_a(amount_in, 0).is_ok() {
                prop_assert!(pool2.reserve_a > 0);
                prop_assert!(pool2.reserve_b > 0);
            }
        }

        /// Add liquidity always increases (or maintains) both reserves.
        #[test]
        fn add_liquidity_increases_reserves(
            ra in 1_000_000u128..=1_000_000_000u128,
            rb in 1_000_000u128..=1_000_000_000u128,
            // Add at the same 1:1 ratio as an initial deposit to avoid ratio mismatch
            add_a in 1_000u128..=100_000u128,
        ) {
            let mut pool = LiquidityPool::new(
                H256::zero(),
                Address::from_slice(&[1u8; 20]).unwrap(),
                Address::from_slice(&[2u8; 20]).unwrap(),
                30,
            ).unwrap();
            // Initial deposit
            pool.add_liquidity(ra, rb, 0).unwrap();
            let ra_before = pool.reserve_a;
            let rb_before = pool.reserve_b;

            // Compute add_b proportionally so the ratio check passes
            // add_b/add_a == rb/ra  =>  add_b = add_a * rb / ra
            if ra == 0 { return Ok(()); }
            let add_b = add_a
                .checked_mul(rb)
                .map(|x| x / ra);
            if let Some(add_b) = add_b {
                if add_b == 0 { return Ok(()); }
                // Only assert when add_liquidity succeeds (ratio must match exactly)
                if let Ok(_lp) = pool.add_liquidity(add_a, add_b, 0) {
                    prop_assert!(pool.reserve_a >= ra_before);
                    prop_assert!(pool.reserve_b >= rb_before);
                }
            }
        }

        /// Remove liquidity always decreases both reserves.
        #[test]
        fn remove_liquidity_decreases_reserves(
            ra in 1_000_000u128..=1_000_000_000u128,
            rb in 1_000_000u128..=1_000_000_000u128,
        ) {
            let mut pool = LiquidityPool::new(
                H256::zero(),
                Address::from_slice(&[1u8; 20]).unwrap(),
                Address::from_slice(&[2u8; 20]).unwrap(),
                30,
            ).unwrap();
            pool.add_liquidity(ra, rb, 0).unwrap();
            let lp = pool.lp_token_supply;
            let ra_before = pool.reserve_a;
            let rb_before = pool.reserve_b;

            // Remove half the LP tokens
            let remove = (lp / 2).max(1);
            if remove <= lp {
                if let Ok((out_a, out_b)) = pool.remove_liquidity(remove, 0, 0) {
                    prop_assert!(pool.reserve_a <= ra_before);
                    prop_assert!(pool.reserve_b <= rb_before);
                    prop_assert!(out_a <= ra_before);
                    prop_assert!(out_b <= rb_before);
                }
            }
        }

        /// Swap output is non-negative and bounded: 0 <= out < reserve_out.
        #[test]
        fn swap_output_bounded(
            ra in arb_reserve(),
            rb in arb_reserve(),
            pct in 1u64..=9u64,  // 1-9% of reserve as input
        ) {
            let amount_in = ((ra as u64 / 10) * pct).max(1) as u128;
            let mut pool = seeded_pool(ra, rb, 30);
            if let Ok(out) = pool.swap_a_to_b(amount_in, 0) {
                prop_assert!(out > 0, "swap should produce non-zero output");
                prop_assert!(out < rb, "swap output must be < reserve_out");
            }
        }

        /// Two sequential swaps in opposite directions result in
        /// the first token amount being slightly less than start (fees consumed).
        #[test]
        fn round_trip_swap_loses_to_fees(
            ra in 1_000_000u128..=100_000_000u128,
            rb in 1_000_000u128..=100_000_000u128,
            amount_in in 1_000u128..=10_000u128,
        ) {
            let mut pool = seeded_pool(ra, rb, 30);
            // Swap A→B
            let out_b = match pool.swap_a_to_b(amount_in, 0) {
                Ok(v) => v,
                Err(_) => return Ok(()),
            };
            // Swap B→A with the B we got back
            let out_a = match pool.swap_b_to_a(out_b, 0) {
                Ok(v) => v,
                Err(_) => return Ok(()),
            };
            // Due to fees and integer rounding, we always get back less
            prop_assert!(out_a < amount_in,
                "round-trip should lose tokens to fees: in={amount_in} out={out_a}");
        }
    }
}
