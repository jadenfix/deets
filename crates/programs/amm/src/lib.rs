// ============================================================================
// AETHER AMM DEX - Automated Market Maker
// ============================================================================
// PURPOSE: Decentralized token swaps using constant product formula
//
// FORMULA: x * y = k
// - x, y = token reserves
// - k = constant product invariant
// - Must hold: k_after >= k_before (fees increase k)
//
// OPERATIONS:
// - add_liquidity: Deposit tokens, receive LP tokens
// - remove_liquidity: Burn LP tokens, receive tokens
// - swap_a_to_b: Exchange token A for B
// - swap_b_to_a: Exchange token B for A
//
// PRICING:
// - Price = reserve_b / reserve_a
// - Slippage increases with trade size
// - Price impact = Δk / k
//
// FEES:
// - Swap fee: 0.3% (30 basis points)
// - Collected in reserves (increases k)
// - Distributed to LP providers
//
// SECURITY:
// - Slippage protection (min_amount_out)
// - Invariant checks after swaps
// - Rounding favors pool
// ============================================================================

pub mod pool;

pub use pool::LiquidityPool;
