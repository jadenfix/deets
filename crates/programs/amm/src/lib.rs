// ============================================================================
// AETHER AMM PROGRAM - Constant Product Market Maker (DEX)
// ============================================================================
// PURPOSE: Decentralized exchange for AIC, SWR, and other tokens
//
// ALGORITHM: Constant Product (x * y = k)
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    AMM DEX                                        │
// ├──────────────────────────────────────────────────────────────────┤
// │  User Swap  →  Pool Reserves  →  Price Calculation (x*y=k)       │
// │         ↓                              ↓                          │
// │  Fee Deduction  →  Reserve Update  →  LP Fee Accumulation        │
// │         ↓                              ↓                          │
// │  Add Liquidity  →  Mint LP Tokens  →  Proportional Shares        │
// │         ↓                              ↓                          │
// │  Remove Liquidity  →  Burn LP Tokens  →  Redeem Proportional     │
// └──────────────────────────────────────────────────────────────────┘
//
// POOL STATE:
// ```
// struct Pool:
//     token_a: TokenId
//     token_b: TokenId
//     reserve_a: u128
//     reserve_b: u128
//     lp_token: TokenId
//     lp_total_supply: u128
//     fee_bps: u32  // Basis points (e.g., 30 = 0.3%)
//     accumulated_fees_a: u128
//     accumulated_fees_b: u128
// ```
//
// SWAP:
// ```
// fn swap(pool_id, token_in, amount_in, min_amount_out):
//     pool = get_pool(pool_id)
//     
//     // Determine reserves
//     if token_in == pool.token_a:
//         (reserve_in, reserve_out) = (pool.reserve_a, pool.reserve_b)
//     else:
//         (reserve_in, reserve_out) = (pool.reserve_b, pool.reserve_a)
//     
//     // Calculate output (constant product formula)
//     // (x + dx) * (y - dy) = x * y
//     // dy = y * dx / (x + dx)
//     // With fee: dx_after_fee = dx * (10000 - fee_bps) / 10000
//     
//     fee_multiplier = 10_000 - pool.fee_bps
//     amount_in_with_fee = amount_in * fee_multiplier / 10_000
//     
//     amount_out = (amount_in_with_fee * reserve_out) / (reserve_in + amount_in_with_fee)
//     
//     require(amount_out >= min_amount_out, "slippage exceeded")
//     
//     // Update reserves
//     if token_in == pool.token_a:
//         pool.reserve_a += amount_in
//         pool.reserve_b -= amount_out
//     else:
//         pool.reserve_b += amount_in
//         pool.reserve_a -= amount_out
//     
//     // Transfer tokens
//     transfer_token(caller, pool_account, token_in, amount_in)
//     transfer_token(pool_account, caller, token_out, amount_out)
//     
//     return amount_out
// ```
//
// ADD LIQUIDITY:
// ```
// fn add_liquidity(pool_id, amount_a, amount_b, min_lp_tokens):
//     pool = get_pool(pool_id)
//     
//     // Calculate LP tokens to mint
//     // lp_tokens = min(amount_a / reserve_a, amount_b / reserve_b) * lp_supply
//     
//     if pool.lp_total_supply == 0:
//         // Initial liquidity
//         lp_tokens = sqrt(amount_a * amount_b)
//     else:
//         lp_a = amount_a * pool.lp_total_supply / pool.reserve_a
//         lp_b = amount_b * pool.lp_total_supply / pool.reserve_b
//         lp_tokens = min(lp_a, lp_b)
//     
//     require(lp_tokens >= min_lp_tokens, "insufficient liquidity")
//     
//     // Update reserves
//     pool.reserve_a += amount_a
//     pool.reserve_b += amount_b
//     
//     // Mint LP tokens
//     pool.lp_total_supply += lp_tokens
//     mint_token(pool.lp_token, caller, lp_tokens)
//     
//     // Transfer tokens to pool
//     transfer_token(caller, pool_account, pool.token_a, amount_a)
//     transfer_token(caller, pool_account, pool.token_b, amount_b)
//     
//     return lp_tokens
// ```
//
// REMOVE LIQUIDITY:
// ```
// fn remove_liquidity(pool_id, lp_tokens, min_amount_a, min_amount_b):
//     pool = get_pool(pool_id)
//     
//     // Calculate token amounts
//     // amount_a = lp_tokens * reserve_a / lp_supply
//     // amount_b = lp_tokens * reserve_b / lp_supply
//     
//     amount_a = lp_tokens * pool.reserve_a / pool.lp_total_supply
//     amount_b = lp_tokens * pool.reserve_b / pool.lp_total_supply
//     
//     require(amount_a >= min_amount_a && amount_b >= min_amount_b)
//     
//     // Update reserves
//     pool.reserve_a -= amount_a
//     pool.reserve_b -= amount_b
//     
//     // Burn LP tokens
//     pool.lp_total_supply -= lp_tokens
//     burn_token(pool.lp_token, caller, lp_tokens)
//     
//     // Transfer tokens to user
//     transfer_token(pool_account, caller, pool.token_a, amount_a)
//     transfer_token(pool_account, caller, pool.token_b, amount_b)
//     
//     return (amount_a, amount_b)
// ```
//
// INVARIANT:
// Constant product: reserve_a * reserve_b = k
// After swap: (reserve_a + dx) * (reserve_b - dy) >= k
// (Equality if no fees; inequality accounts for fee accumulation)
//
// MATH (fixed-point):
// Use Q64.64 fixed-point arithmetic for precision:
//   - Avoid overflow: intermediate results fit in u256
//   - Rounding: always round in pool's favor (prevent drain attacks)
//
// SLIPPAGE PROTECTION:
// - min_amount_out on swaps
// - min_lp_tokens on add liquidity
// - min_amount_a/b on remove liquidity
//
// OUTPUTS:
// - AIC/SWR liquidity → Enables trading
// - LP tokens → Represent liquidity shares (eUTxO-based)
// - Fee accumulation → Passive income for LPs
// ============================================================================

pub mod pool;
pub mod swap;
pub mod liquidity;
pub mod math;

pub use pool::Pool;
pub use math::constant_product_swap;

