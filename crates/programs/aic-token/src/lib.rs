// ============================================================================
// AETHER AIC TOKEN PROGRAM - AI Credits Token Management
// ============================================================================
// PURPOSE: Manage AIC token supply, transfers, and burning mechanism
//
// TOKEN: AIC (AI Credits) - utility token for AI compute, burned on use
//
// COMPONENT CONNECTIONS:
// ┌──────────────────────────────────────────────────────────────────┐
// │                    AIC TOKEN SYSTEM                               │
// ├──────────────────────────────────────────────────────────────────┤
// │  User Transfer  →  Balance Update  →  Account State              │
// │         ↓                                ↓                        │
// │  Job Escrow  →  AIC Burn  →  Supply Decrease (deflationary)      │
// │         ↓                                ↓                        │
// │  Provider Payment  →  Treasury/Stablecoin  →  Separate from AIC  │
// └──────────────────────────────────────────────────────────────────┘
//
// ACCOUNT STATE:
// ```
// struct AicAccount:
//     address: Address
//     balance: u128
//     frozen: bool
//     allowances: HashMap<Address, u128>  // For delegated transfers
// ```
//
// OPERATIONS:
// ```
// fn transfer(to, amount):
//     from_account = get_account(caller)
//     require(!from_account.frozen)
//     require(from_account.balance >= amount)
//     
//     from_account.balance -= amount
//     
//     to_account = get_or_create_account(to)
//     to_account.balance += amount
//     
//     emit_event(Transfer { from: caller, to: to, amount: amount })
//
// fn burn(amount):
//     account = get_account(caller)
//     require(account.balance >= amount)
//     
//     account.balance -= amount
//     total_supply -= amount
//     
//     emit_event(Burn { from: caller, amount: amount })
//
// fn approve(spender, amount):
//     account = get_account(caller)
//     account.allowances[spender] = amount
//     
//     emit_event(Approval { owner: caller, spender: spender, amount: amount })
//
// fn transfer_from(from, to, amount):
//     from_account = get_account(from)
//     require(!from_account.frozen)
//     require(from_account.allowances[caller] >= amount)
//     require(from_account.balance >= amount)
//     
//     from_account.balance -= amount
//     from_account.allowances[caller] -= amount
//     
//     to_account = get_or_create_account(to)
//     to_account.balance += amount
//     
//     emit_event(Transfer { from: from, to: to, amount: amount })
// ```
//
// PRIVILEGED OPERATIONS (Job Escrow Program only):
// ```
// fn escrow_burn(account, amount):
//     // Only callable by Job Escrow Program
//     require(caller == JOB_ESCROW_PROGRAM)
//     
//     acc = get_account(account)
//     require(acc.balance >= amount)
//     
//     acc.balance -= amount
//     total_supply -= amount
//     
//     emit_event(Burn { from: account, amount: amount })
//
// fn escrow_transfer(from, to, amount):
//     // Only callable by Job Escrow Program
//     require(caller == JOB_ESCROW_PROGRAM)
//     
//     from_acc = get_account(from)
//     require(from_acc.balance >= amount)
//     
//     from_acc.balance -= amount
//     
//     to_acc = get_or_create_account(to)
//     to_acc.balance += amount
//     
//     emit_event(Transfer { from: from, to: to, amount: amount })
// ```
//
// TOKENOMICS:
// - Initial Supply: 10B AIC (genesis allocation)
// - Decimals: 6 (1 AIC = 1,000,000 micro-AIC)
// - Deflationary: Burned on AI job completion
// - Provider Payment: Separate stablecoin/token from treasury
// - No minting: Fixed initial supply (burns only)
//
// BURN MECHANISM:
// When AI job completes successfully:
//   1. User's escrowed AIC is burned (deflationary pressure)
//   2. Provider receives payment from separate treasury (stablecoin/AIC)
//   3. Total AIC supply decreases over time
//
// SUPPLY TRACKING:
// ```
// struct SupplyMetrics:
//     total_supply: u128
//     total_burned: u128
//     circulating_supply: u128  // total - burned
//     burn_rate_per_epoch: u128  // EWMA
// ```
//
// ACCOUNT FREEZING:
// Governance can freeze accounts (e.g., sanctioned addresses):
// ```
// fn freeze_account(address):
//     require(caller == GOVERNANCE_PROGRAM)
//     account = get_account(address)
//     account.frozen = true
// ```
//
// OUTPUTS:
// - Balance updates → User wallets
// - Burn events → Supply tracking & analytics
// - Transfer events → Indexer & block explorer
// ============================================================================

pub mod token;
pub mod transfer;
pub mod burn;
pub mod allowance;

pub use token::AicAccount;

