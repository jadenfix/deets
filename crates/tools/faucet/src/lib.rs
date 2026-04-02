use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use aether_types::primitives::H160;

#[derive(Debug, Clone)]
pub struct FaucetConfig {
    pub default_amount_limit: u64,
    pub cooldown: Duration,
    pub token_allowlist: Vec<String>,
}

impl Default for FaucetConfig {
    fn default() -> Self {
        FaucetConfig {
            default_amount_limit: 250_000,
            cooldown: Duration::from_secs(60 * 10),
            token_allowlist: vec!["AIC".to_string(), "SWR".to_string()],
        }
    }
}

#[derive(Clone)]
struct AppState {
    config: FaucetConfig,
    last_requests: Arc<Mutex<HashMap<String, Instant>>>,
}

impl AppState {
    fn new(config: FaucetConfig) -> Self {
        AppState {
            config,
            last_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FaucetRequest {
    pub github: String,
    pub address: String,
    pub token: String,
    pub amount: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FaucetGrant {
    pub address: String,
    pub token: String,
    pub amount: u64,
    pub memo: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FaucetResponse {
    pub status: String,
    pub message: String,
    pub grant: Option<FaucetGrant>,
}

#[derive(Debug, Error)]
enum FaucetError {
    #[error("github handle is required")]
    MissingGithub,
    #[error("github handle invalid: must start and end with alphanumeric, may contain hyphens, and be 1-39 characters (e.g. 'octocat')")]
    InvalidGithub,
    #[error("address invalid: expected a 0x-prefixed 40-character hex address (e.g. 0x1111111111111111111111111111111111111111)")]
    InvalidAddress,
    #[error("token not allowed: allowed tokens are {0}")]
    TokenNotAllowed(String),
    #[error("amount must be between 1 and {0} (the per-request faucet limit)")]
    AmountLimit(u64),
    #[error("request throttled: try again in {0} seconds")]
    Throttled(u64),
}

static GITHUB_HANDLE_RE: OnceLock<regex::Regex> = OnceLock::new();

fn github_handle_re() -> &'static regex::Regex {
    GITHUB_HANDLE_RE.get_or_init(|| {
        regex::Regex::new(r"^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,38}[a-zA-Z0-9])?$").unwrap()
    })
}

fn validate_github(handle: &str) -> Result<(), FaucetError> {
    if handle.trim().is_empty() {
        return Err(FaucetError::MissingGithub);
    }
    if github_handle_re().is_match(handle) {
        Ok(())
    } else {
        Err(FaucetError::InvalidGithub)
    }
}

fn parse_address(address: &str) -> Result<H160, FaucetError> {
    let trimmed = address.strip_prefix("0x").unwrap_or(address);
    let bytes = hex::decode(trimmed).map_err(|_| FaucetError::InvalidAddress)?;
    H160::from_slice(&bytes).map_err(|_| FaucetError::InvalidAddress)
}

fn validate_amount(amount: u64, limit: u64) -> Result<(), FaucetError> {
    if amount == 0 || amount > limit {
        Err(FaucetError::AmountLimit(limit))
    } else {
        Ok(())
    }
}

fn validate_token(token: &str, allowlist: &[String]) -> Result<(), FaucetError> {
    if allowlist
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(token))
    {
        Ok(())
    } else {
        Err(FaucetError::TokenNotAllowed(allowlist.join(", ")))
    }
}

fn check_rate_limit(state: &AppState, handle: &str) -> Result<(), FaucetError> {
    let mut map = state.last_requests.lock();
    let now = Instant::now();
    if let Some(last) = map.get(handle) {
        let elapsed = now.duration_since(*last);
        if elapsed < state.config.cooldown {
            let remaining = (state.config.cooldown - elapsed).as_secs();
            return Err(FaucetError::Throttled(remaining));
        }
    }
    map.insert(handle.to_string(), now);
    Ok(())
}

async fn handle_request(
    State(state): State<AppState>,
    Json(payload): Json<FaucetRequest>,
) -> (StatusCode, Json<FaucetResponse>) {
    match process_request(&state, payload) {
        Ok(grant) => (
            StatusCode::OK,
            Json(FaucetResponse {
                status: "accepted".to_string(),
                message: "request accepted".to_string(),
                grant: Some(grant),
            }),
        ),
        Err(err) => {
            let status = match err {
                FaucetError::Throttled(_) => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::BAD_REQUEST,
            };
            (
                status,
                Json(FaucetResponse {
                    status: "rejected".to_string(),
                    message: err.to_string(),
                    grant: None,
                }),
            )
        }
    }
}

fn process_request(state: &AppState, payload: FaucetRequest) -> Result<FaucetGrant, FaucetError> {
    validate_github(&payload.github)?;
    let address = parse_address(&payload.address)?;
    validate_token(&payload.token, &state.config.token_allowlist)?;
    let limit = state.config.default_amount_limit;
    let amount = payload.amount.unwrap_or(limit);
    validate_amount(amount, limit)?;
    check_rate_limit(state, &payload.github)?;

    let memo = format!("faucet:{}:{}", payload.token.to_uppercase(), payload.github);
    Ok(FaucetGrant {
        address: format!("0x{}", hex::encode(address.as_bytes())),
        token: payload.token.to_uppercase(),
        amount,
        memo,
    })
}

pub fn faucet_app(config: FaucetConfig) -> Router {
    let state = AppState::new(config);
    Router::new()
        .route("/request", post(handle_request))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> Router {
        faucet_app(FaucetConfig {
            default_amount_limit: 100,
            cooldown: Duration::from_secs(5),
            token_allowlist: vec!["AIC".into()],
        })
    }

    fn request_json(body: &FaucetRequest) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/request")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    const BODY_LIMIT: usize = 32 * 1024;

    #[tokio::test]
    async fn accepts_valid_request() {
        let app = test_state();
        let req = FaucetRequest {
            github: "aetherdev".into(),
            address: "0x".to_string() + &"11".repeat(20),
            token: "AIC".into(),
            amount: Some(80),
        };

        let response = app.clone().oneshot(request_json(&req)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: FaucetResponse =
            serde_json::from_slice(&to_bytes(response.into_body(), BODY_LIMIT).await.unwrap())
                .unwrap();
        assert_eq!(body.status, "accepted");
        assert!(body.grant.is_some());
        assert_eq!(body.grant.unwrap().amount, 80);
    }

    #[tokio::test]
    async fn rejects_rate_limit() {
        let app = test_state();
        let req = FaucetRequest {
            github: "rate-limited".into(),
            address: "0x".to_string() + &"22".repeat(20),
            token: "AIC".into(),
            amount: Some(50),
        };

        let _ = app.clone().oneshot(request_json(&req)).await.unwrap();
        let response = app.clone().oneshot(request_json(&req)).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn rejects_invalid_github() {
        let app = test_state();
        let req = FaucetRequest {
            github: "".into(),
            address: "0x".to_string() + &"33".repeat(20),
            token: "AIC".into(),
            amount: None,
        };

        let response = app.clone().oneshot(request_json(&req)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rejects_unknown_token() {
        let app = test_state();
        let req = FaucetRequest {
            github: "meshbuilder".into(),
            address: "0x".to_string() + &"44".repeat(20),
            token: "XYZ".into(),
            amount: Some(10),
        };

        let response = app.clone().oneshot(request_json(&req)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // ── strategies ───────────────────────────────────────────────────────────

    /// Generate a valid GitHub handle: starts/ends with alphanumeric, may
    /// contain interior hyphens, 1-39 characters total.
    fn valid_github() -> impl Strategy<Value = String> {
        // Single char: just alphanumeric
        let single = "[a-zA-Z0-9]".prop_map(|s: String| s);
        // Two or more chars: first + optional middle + last
        let multi = (
            "[a-zA-Z0-9]",
            prop::collection::vec("[a-zA-Z0-9-]", 0..37usize),
            "[a-zA-Z0-9]",
        )
            .prop_map(|(first, middle, last)| {
                let mut s = first;
                for c in middle {
                    s.push_str(&c);
                }
                s.push_str(&last);
                s
            });
        prop_oneof![single, multi]
    }

    /// Generate a valid 0x-prefixed 40-hex-char address.
    fn valid_address() -> impl Strategy<Value = String> {
        prop::array::uniform20(any::<u8>())
            .prop_map(|bytes| format!("0x{}", hex::encode(bytes)))
    }

    /// Generate a valid amount in [1, 250_000].
    fn valid_amount() -> impl Strategy<Value = u64> {
        1u64..=250_000u64
    }

    // ── property tests ────────────────────────────────────────────────────────

    proptest! {
        /// Any string that matches the GitHub regex is accepted by validate_github.
        #[test]
        fn valid_github_handle_accepted(handle in valid_github()) {
            prop_assume!(handle.len() <= 39);
            prop_assert!(validate_github(&handle).is_ok(),
                "expected {:?} to be accepted", handle);
        }

        /// Empty and whitespace-only strings are rejected.
        #[test]
        fn empty_github_handle_rejected(spaces in " +") {
            prop_assert!(validate_github("").is_err());
            prop_assert!(validate_github(&spaces).is_err());
        }

        /// Handles starting with a hyphen are always rejected.
        #[test]
        fn github_handle_starting_with_hyphen_rejected(
            suffix in "[a-zA-Z0-9]{1,38}"
        ) {
            let handle = format!("-{}", suffix);
            prop_assert!(validate_github(&handle).is_err(),
                "expected {:?} to be rejected", handle);
        }

        /// Handles ending with a hyphen are always rejected.
        #[test]
        fn github_handle_ending_with_hyphen_rejected(
            prefix in "[a-zA-Z0-9]{1,38}"
        ) {
            let handle = format!("{}-", prefix);
            prop_assert!(validate_github(&handle).is_err(),
                "expected {:?} to be rejected", handle);
        }

        /// Valid 0x-prefixed 40-hex-char addresses always parse successfully.
        #[test]
        fn valid_hex_address_accepted(addr in valid_address()) {
            prop_assert!(parse_address(&addr).is_ok(),
                "expected {:?} to be accepted", addr);
        }

        /// Hex addresses without the 0x prefix are still accepted.
        #[test]
        fn address_without_0x_prefix_accepted(bytes in prop::array::uniform20(any::<u8>())) {
            let addr = hex::encode(bytes);
            prop_assert!(parse_address(&addr).is_ok());
        }

        /// Non-hex strings in the address field are always rejected.
        #[test]
        fn non_hex_address_rejected(junk in "[g-z]{40}") {
            // letters g-z are not valid hex digits
            let addr = format!("0x{}", junk);
            prop_assert!(parse_address(&addr).is_err(), "addr {:?} should be rejected", addr);
        }

        /// Amounts in [1, limit] are accepted; 0 or over limit are rejected.
        #[test]
        fn valid_amount_accepted(amount in valid_amount(), limit in 1u64..=1_000_000u64) {
            prop_assume!(amount <= limit);
            prop_assert!(validate_amount(amount, limit).is_ok());
        }

        #[test]
        fn zero_amount_rejected(limit in 1u64..=1_000_000u64) {
            prop_assert!(validate_amount(0, limit).is_err());
        }

        #[test]
        fn over_limit_amount_rejected(limit in 1u64..=999_999u64, excess in 1u64..=1000u64) {
            prop_assert!(validate_amount(limit + excess, limit).is_err());
        }

        /// Tokens in the allowlist (case-insensitive) are accepted.
        #[test]
        fn allowlisted_token_accepted(
            token in prop_oneof!["AIC", "SWR", "aic", "swr", "Aic", "Swr"]
        ) {
            let allowlist = vec!["AIC".to_string(), "SWR".to_string()];
            prop_assert!(validate_token(&token, &allowlist).is_ok());
        }

        /// Random strings that are not AIC or SWR are rejected.
        #[test]
        fn unlisted_token_rejected(token in "[A-Z]{3,6}") {
            prop_assume!(token != "AIC" && token != "SWR");
            let allowlist = vec!["AIC".to_string(), "SWR".to_string()];
            prop_assert!(validate_token(&token, &allowlist).is_err());
        }

        /// The faucet grant memo always encodes the uppercase token and github handle.
        #[test]
        fn grant_memo_encodes_token_and_handle(
            handle in valid_github(),
            token in prop_oneof!["AIC", "SWR"],
            bytes in prop::array::uniform20(any::<u8>()),
        ) {
            prop_assume!(handle.len() <= 39);
            let state = AppState::new(FaucetConfig::default());
            let address = format!("0x{}", hex::encode(bytes));
            let payload = FaucetRequest {
                github: handle.clone(),
                address,
                token: token.to_string(),
                amount: Some(1),
            };
            let grant = process_request(&state, payload).unwrap();
            prop_assert!(grant.memo.contains(&token.to_uppercase()));
            prop_assert!(grant.memo.contains(&handle));
        }

        /// The returned address is always lowercase hex with 0x prefix.
        #[test]
        fn grant_address_is_normalized(
            handle in valid_github(),
            bytes in prop::array::uniform20(any::<u8>()),
        ) {
            prop_assume!(handle.len() <= 39);
            let state = AppState::new(FaucetConfig::default());
            let address = format!("0x{}", hex::encode(bytes));
            let payload = FaucetRequest {
                github: handle,
                address: address.clone(),
                token: "AIC".to_string(),
                amount: Some(1),
            };
            let grant = process_request(&state, payload).unwrap();
            prop_assert!(grant.address.starts_with("0x"));
            prop_assert_eq!(grant.address.len(), 42);
            prop_assert!(grant.address[2..].chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// Granted amount equals the requested amount when within limits.
        #[test]
        fn grant_amount_matches_requested(
            handle in valid_github(),
            bytes in prop::array::uniform20(any::<u8>()),
            amount in 1u64..=250_000u64,
        ) {
            prop_assume!(handle.len() <= 39);
            let state = AppState::new(FaucetConfig::default());
            let payload = FaucetRequest {
                github: handle,
                address: format!("0x{}", hex::encode(bytes)),
                token: "AIC".to_string(),
                amount: Some(amount),
            };
            let grant = process_request(&state, payload).unwrap();
            prop_assert_eq!(grant.amount, amount);
        }
    }
}
