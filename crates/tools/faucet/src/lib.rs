use std::collections::HashMap;
use std::sync::Arc;
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
    #[error("github handle invalid")]
    InvalidGithub,
    #[error("address invalid")]
    InvalidAddress,
    #[error("token not allowed")]
    TokenNotAllowed,
    #[error("amount exceeds limit ({0})")]
    AmountLimit(u64),
    #[error("request throttled – retry later")]
    Throttled,
}

fn validate_github(handle: &str) -> Result<(), FaucetError> {
    if handle.trim().is_empty() {
        return Err(FaucetError::MissingGithub);
    }
    let re = regex::Regex::new(r"^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,38}[a-zA-Z0-9])?$").unwrap();
    if re.is_match(handle) {
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
        Err(FaucetError::TokenNotAllowed)
    }
}

fn check_rate_limit(state: &AppState, handle: &str) -> Result<(), FaucetError> {
    let mut map = state.last_requests.lock();
    let now = Instant::now();
    if let Some(last) = map.get(handle) {
        if now.duration_since(*last) < state.config.cooldown {
            return Err(FaucetError::Throttled);
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
                FaucetError::Throttled => StatusCode::TOO_MANY_REQUESTS,
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
