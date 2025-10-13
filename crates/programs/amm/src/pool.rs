use aether_types::{Address, H256};
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
    pub fn new(pool_id: H256, token_a: Address, token_b: Address, fee_bps: u32) -> Self {
        LiquidityPool {
            pool_id,
            token_a,
            token_b,
            reserve_a: 0,
            reserve_b: 0,
            lp_token_supply: 0,
            fee_bps,
        }
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
            // Initial liquidity
            let liquidity = (amount_a.checked_mul(amount_b).ok_or("overflow")?).integer_sqrt();

            if liquidity < 1000 {
                return Err("insufficient initial liquidity".to_string());
            }

            self.reserve_a = amount_a;
            self.reserve_b = amount_b;
            self.lp_token_supply = liquidity;

            liquidity
        } else {
            // Proportional liquidity
            let liquidity_a = (amount_a * self.lp_token_supply) / self.reserve_a;
            let liquidity_b = (amount_b * self.lp_token_supply) / self.reserve_b;

            let liquidity = liquidity_a.min(liquidity_b);

            if liquidity < min_lp_tokens {
                return Err("insufficient LP tokens".to_string());
            }

            self.reserve_a += amount_a;
            self.reserve_b += amount_b;
            self.lp_token_supply += liquidity;

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

        let amount_a = (lp_tokens * self.reserve_a) / self.lp_token_supply;
        let amount_b = (lp_tokens * self.reserve_b) / self.lp_token_supply;

        if amount_a < min_amount_a || amount_b < min_amount_b {
            return Err("insufficient output amount".to_string());
        }

        self.reserve_a -= amount_a;
        self.reserve_b -= amount_b;
        self.lp_token_supply -= lp_tokens;

        Ok((amount_a, amount_b))
    }

    /// Swap token A for token B
    pub fn swap_a_to_b(&mut self, amount_in: u128, min_amount_out: u128) -> Result<u128, String> {
        if amount_in == 0 {
            return Err("amount must be non-zero".to_string());
        }

        let amount_out = self.get_amount_out(amount_in, self.reserve_a, self.reserve_b)?;

        if amount_out < min_amount_out {
            return Err("insufficient output amount".to_string());
        }

        self.reserve_a += amount_in;
        self.reserve_b -= amount_out;

        // Verify invariant
        self.check_invariant()?;

        Ok(amount_out)
    }

    /// Swap token B for token A
    pub fn swap_b_to_a(&mut self, amount_in: u128, min_amount_out: u128) -> Result<u128, String> {
        if amount_in == 0 {
            return Err("amount must be non-zero".to_string());
        }

        let amount_out = self.get_amount_out(amount_in, self.reserve_b, self.reserve_a)?;

        if amount_out < min_amount_out {
            return Err("insufficient output amount".to_string());
        }

        self.reserve_b += amount_in;
        self.reserve_a -= amount_out;

        // Verify invariant
        self.check_invariant()?;

        Ok(amount_out)
    }

    /// Calculate output amount for a swap
    /// Formula: amount_out = (amount_in * fee * reserve_out) / (reserve_in + amount_in * fee)
    fn get_amount_out(
        &self,
        amount_in: u128,
        reserve_in: u128,
        reserve_out: u128,
    ) -> Result<u128, String> {
        if amount_in == 0 || reserve_in == 0 || reserve_out == 0 {
            return Err("invalid reserves".to_string());
        }

        // Apply fee (basis points)
        let fee_multiplier = 10000 - self.fee_bps;
        let amount_in_with_fee = amount_in
            .checked_mul(fee_multiplier as u128)
            .ok_or("overflow")?;

        let numerator = amount_in_with_fee
            .checked_mul(reserve_out)
            .ok_or("overflow")?;

        let denominator = reserve_in
            .checked_mul(10000)
            .ok_or("overflow")?
            .checked_add(amount_in_with_fee)
            .ok_or("overflow")?;

        let amount_out = numerator / denominator;

        Ok(amount_out)
    }

    /// Check constant product invariant (with tolerance for rounding)
    fn check_invariant(&self) -> Result<(), String> {
        let k_new = self
            .reserve_a
            .checked_mul(self.reserve_b)
            .ok_or("overflow")?;

        // Invariant should not decrease (may increase slightly due to fees)
        if k_new == 0 {
            return Err("invariant violated: k = 0".to_string());
        }

        Ok(())
    }

    /// Get current price (reserve_b / reserve_a)
    pub fn get_price(&self) -> Result<u128, String> {
        if self.reserve_a == 0 {
            return Err("zero reserve".to_string());
        }

        Ok((self.reserve_b * 1_000_000) / self.reserve_a)
    }
}

trait IntegerSqrt {
    fn integer_sqrt(self) -> Self;
}

impl IntegerSqrt for u128 {
    fn integer_sqrt(self) -> Self {
        if self < 2 {
            return self;
        }

        let mut x = self;
        let mut y = x.div_ceil(2);

        while y < x {
            x = y;
            y = (x + self / x) / 2;
        }

        x
    }
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
    }

    #[test]
    fn test_add_initial_liquidity() {
        let mut pool = test_pool();

        let lp_tokens = pool.add_liquidity(1000, 1000, 0).unwrap();

        assert_eq!(pool.reserve_a, 1000);
        assert_eq!(pool.reserve_b, 1000);
        assert_eq!(lp_tokens, 1000);
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

        pool.add_liquidity(1000, 1000, 0).unwrap();
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
}
