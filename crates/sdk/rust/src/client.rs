use aether_types::Transaction;
use anyhow::Context;
use serde::Deserialize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::AetherSdkError;
use crate::job_builder::JobBuilder;
use crate::transaction_builder::TransferBuilder;
use crate::types::{ClientConfig, JobRequest, JobSubmission, SubmitResponse};

#[derive(Clone, Debug)]
pub struct AetherClient {
    endpoint: String,
    config: ClientConfig,
}

impl AetherClient {
    /// Create a new client connected to the given RPC endpoint URL.
    pub fn new(endpoint: impl Into<String>) -> Self {
        AetherClient {
            endpoint: endpoint.into(),
            config: ClientConfig::default(),
        }
    }

    /// Create a new client with a custom configuration.
    pub fn with_config(endpoint: impl Into<String>, config: ClientConfig) -> Self {
        AetherClient {
            endpoint: endpoint.into(),
            config,
        }
    }

    /// Return the RPC endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Return a reference to the client configuration.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Start building a token transfer transaction.
    pub fn transfer(&self) -> TransferBuilder {
        TransferBuilder::new(&self.config)
    }

    /// Start building an AI job submission.
    pub fn job(&self) -> JobBuilder {
        JobBuilder::new(&self.endpoint)
    }

    /// Submit a signed transaction to the network.
    ///
    /// Returns a typed [`AetherSdkError`] so callers can match on specific
    /// failure modes (invalid signature, network I/O, RPC error, hash
    /// mismatch, …) without string inspection.
    pub async fn submit(&self, tx: Transaction) -> Result<SubmitResponse, AetherSdkError> {
        tx.verify_signature()
            .map_err(|e| AetherSdkError::InvalidSignature(e.to_string()))?;
        let fee_params = aether_types::ChainConfig::devnet().fees;
        tx.calculate_fee(&fee_params)
            .map_err(|e| AetherSdkError::InvalidFee(e.to_string()))?;

        let tx_hash = tx.hash();
        let payload = SubmitRpcRequest::new(tx).map_err(AetherSdkError::serialization)?;
        let endpoint = HttpEndpoint::parse(&self.endpoint)?;
        let body = serde_json::to_vec(&payload).map_err(AetherSdkError::serialization)?;
        let headers = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            endpoint.path,
            endpoint.host_header(),
            body.len()
        );

        let response_text = self.rpc_request(&endpoint, &headers, &body).await?;
        let (status_line, rpc_body) = parse_http_response(&response_text)?;
        if !status_line.contains(" 200 ") {
            return Err(AetherSdkError::invalid_response(format!(
                "rpc returned non-success status: {status_line}"
            )));
        }

        let response: JsonRpcResponse<String> = serde_json::from_str(rpc_body).map_err(|e| {
            AetherSdkError::invalid_response(format!("failed to decode rpc response body: {e}"))
        })?;

        if let Some(error) = response.error {
            return Err(AetherSdkError::Rpc {
                code: error.code,
                message: error.message,
            });
        }

        let result_hash = response
            .result
            .ok_or_else(|| AetherSdkError::invalid_response("rpc response missing result"))?;
        let returned_hash = parse_h256_hex(&result_hash)?;

        if returned_hash != tx_hash {
            return Err(AetherSdkError::TxHashMismatch {
                expected: format!("{tx_hash:?}"),
                got: format!("{returned_hash:?}"),
            });
        }

        Ok(SubmitResponse {
            tx_hash: returned_hash,
            accepted: true,
        })
    }

    /// Send a raw HTTP/1.1 JSON-RPC request and return the response body.
    ///
    /// Both the TCP connect phase and the response-read phase are wrapped in
    /// `tokio::time::timeout` using [`ClientConfig::request_timeout_secs`].
    /// A stalled or silently-dropped connection therefore cannot block a
    /// tokio task indefinitely.
    ///
    /// This is the single place where all network I/O happens in the SDK.
    /// Every public method that needs to talk to the RPC endpoint should go
    /// through here so timeout enforcement is consistent.
    async fn rpc_request(
        &self,
        endpoint: &HttpEndpoint,
        headers: &str,
        body: &[u8],
    ) -> Result<String, AetherSdkError> {
        let timeout_dur = Duration::from_secs(self.config.request_timeout_secs);

        // Concatenate headers + body in one buffer to avoid partial-read
        // issues on simple HTTP servers that do a single recv() call.
        let mut payload = Vec::with_capacity(headers.len() + body.len());
        payload.extend_from_slice(headers.as_bytes());
        payload.extend_from_slice(body);

        let mut stream = tokio::time::timeout(
            timeout_dur,
            TcpStream::connect((endpoint.host.as_str(), endpoint.port)),
        )
        .await
        .map_err(|_| {
            AetherSdkError::Timeout(format!(
                "timed out connecting to {} after {}s",
                self.endpoint, self.config.request_timeout_secs
            ))
        })?
        .map_err(|e| {
            AetherSdkError::network(format!("failed to connect to {}: {e}", self.endpoint))
        })?;

        stream
            .write_all(&payload)
            .await
            .map_err(|e| AetherSdkError::network(format!("failed to write rpc request: {e}")))?;

        let mut raw = Vec::new();
        tokio::time::timeout(timeout_dur, stream.read_to_end(&mut raw))
            .await
            .map_err(|_| {
                AetherSdkError::Timeout(format!(
                    "timed out reading rpc response from {} after {}s",
                    self.endpoint, self.config.request_timeout_secs
                ))
            })?
            .map_err(|e| AetherSdkError::network(format!("failed to read rpc response: {e}")))?;

        String::from_utf8(raw)
            .map_err(|_| AetherSdkError::invalid_response("rpc response was not valid utf-8"))
    }

    /// Prepare a job submission payload without sending it.
    pub fn prepare_job_submission(&self, job: JobRequest) -> JobSubmission {
        JobSubmission {
            url: format!("{}/v1/jobs", self.endpoint),
            method: "POST".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: job,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct SubmitRpcRequest {
    jsonrpc: &'static str,
    method: &'static str,
    params: Vec<String>,
    id: u64,
}

impl SubmitRpcRequest {
    fn new(tx: Transaction) -> anyhow::Result<Self> {
        let bytes = bincode::serialize(&tx).context("failed to serialize transaction")?;
        Ok(Self {
            jsonrpc: "2.0",
            method: "aeth_sendRawTransaction",
            params: vec![format!("0x{}", hex::encode(bytes))],
            id: 1,
        })
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug)]
struct HttpEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl HttpEndpoint {
    fn parse(endpoint: &str) -> Result<Self, AetherSdkError> {
        let trimmed = endpoint.trim();
        let without_scheme = trimmed.strip_prefix("http://").ok_or_else(|| {
            AetherSdkError::invalid_endpoint(format!(
                "only http:// endpoints are supported, got: {trimmed}"
            ))
        })?;

        let (host_port, path) = if let Some((h, p)) = without_scheme.split_once('/') {
            (h, format!("/{}", p))
        } else {
            (without_scheme, "/".to_string())
        };

        if host_port.is_empty() {
            return Err(AetherSdkError::invalid_endpoint(format!(
                "invalid endpoint host: {endpoint}"
            )));
        }

        let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
            let parsed_port = p.parse::<u16>().map_err(|_| {
                AetherSdkError::invalid_endpoint(format!("invalid endpoint port in {endpoint}"))
            })?;
            (h.to_string(), parsed_port)
        } else {
            (host_port.to_string(), 80)
        };

        if host.is_empty() {
            return Err(AetherSdkError::invalid_endpoint(format!(
                "invalid endpoint host: {endpoint}"
            )));
        }

        Ok(Self { host, port, path })
    }

    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn parse_http_response(response: &str) -> Result<(&str, &str), AetherSdkError> {
    let (headers, body) = response.split_once("\r\n\r\n").ok_or_else(|| {
        AetherSdkError::invalid_response("invalid http response from rpc endpoint")
    })?;
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| AetherSdkError::invalid_response("missing http status line"))?;
    Ok((status_line, body))
}

fn parse_h256_hex(value: &str) -> Result<aether_types::H256, AetherSdkError> {
    let bytes = hex::decode(value.trim_start_matches("0x"))
        .map_err(|_| AetherSdkError::invalid_response(format!("invalid tx hash hex: {value}")))?;
    aether_types::H256::from_slice(&bytes)
        .map_err(|e| AetherSdkError::invalid_response(format!("invalid tx hash: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_types::{Address, PublicKey, Signature, H256};
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn encodes_submit_request_payload() {
        let client = AetherClient::new("http://localhost:8545");
        let keypair = Keypair::generate();
        let recipient = Address::from_slice(&[2u8; 20]).unwrap();
        let tx = client
            .transfer()
            .to(recipient)
            .amount(1_000)
            .memo("sdk test")
            .build(&keypair, 1)
            .unwrap();
        let request = SubmitRpcRequest::new(tx.clone()).unwrap();
        assert_eq!(request.method, "aeth_sendRawTransaction");
        assert_eq!(request.params.len(), 1);

        let encoded = request.params[0].trim_start_matches("0x");
        let decoded = hex::decode(encoded).unwrap();
        let decoded_tx: Transaction = bincode::deserialize(&decoded).unwrap();
        assert_eq!(decoded_tx.hash(), tx.hash());
    }

    #[tokio::test]
    async fn submit_returns_error_for_unreachable_endpoint() {
        let client = AetherClient::new("http://127.0.0.1:1");
        let keypair = Keypair::generate();
        let sender_pubkey = PublicKey::from_bytes(keypair.public_key());
        let sender = sender_pubkey.to_address();
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender,
            sender_pubkey,
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 500_000,
            fee: 2_000_000,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let err = client.submit(tx).await.unwrap_err();
        assert!(
            matches!(err, AetherSdkError::Network(_)),
            "expected Network error, got: {err}"
        );
        assert!(
            err.to_string().contains("failed to connect to"),
            "expected connection error, got: {err}"
        );
    }

    /// Verify that submit() returns Timeout when the server accepts the TCP
    /// connection but never sends a response.  Without timeout enforcement this
    /// test would hang forever; with it the error surfaces within ~1 second.
    #[tokio::test]
    async fn submit_times_out_when_server_hangs() {
        use tokio::net::TcpListener;

        // Bind to an ephemeral port on loopback.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Accept the TCP handshake but never write a response — simulates a
        // stalled node that accepted the connection then stopped responding.
        tokio::spawn(async move {
            if let Ok((_stream, _addr)) = listener.accept().await {
                // Hold the stream open for longer than the client timeout.
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });

        let config = ClientConfig {
            request_timeout_secs: 1, // short timeout so the test finishes quickly
            ..ClientConfig::default()
        };
        let client = AetherClient::with_config(format!("http://127.0.0.1:{port}"), config);

        let keypair = Keypair::generate();
        let sender_pubkey = PublicKey::from_bytes(keypair.public_key());
        let sender = sender_pubkey.to_address();
        let mut tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender,
            sender_pubkey,
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 500_000,
            fee: 2_000_000,
            signature: Signature::from_bytes(vec![]),
        };
        let hash = tx.hash();
        tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));

        let err = client.submit(tx).await.unwrap_err();
        assert!(
            matches!(err, AetherSdkError::Timeout(_)),
            "expected Timeout error when server never responds, got: {err}"
        );
        assert!(
            err.to_string().contains("timed out"),
            "timeout message must contain 'timed out', got: {err}"
        );
    }

    #[test]
    fn builds_job_submission_payload() {
        let client = AetherClient::new("http://localhost:8545");
        let model_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let input_hash = H256::from_slice(&[2u8; 32]).unwrap();

        let submission = client
            .job()
            .job_id("hello-aic-job")
            .unwrap()
            .model_hash(model_hash)
            .input_hash(input_hash)
            .max_fee(500_000_000)
            .expires_at(1_700_000_000)
            .metadata(json!({
                "prompt": "Generate a haiku about verifiable compute.",
                "priority": "gold"
            }))
            .to_submission()
            .unwrap();

        assert_eq!(submission.url, "http://localhost:8545/v1/jobs");
        assert_eq!(submission.method, "POST");
        assert_eq!(
            submission.headers,
            vec![("content-type".to_string(), "application/json".to_string())]
        );
        assert_eq!(submission.body.job_id, "hello-aic-job");

        let prepared = client.prepare_job_submission(submission.body.clone());
        assert_eq!(prepared, submission);
    }

    #[test]
    fn submit_rejects_invalid_signature() {
        // Verify that a bad signature produces AetherSdkError::InvalidSignature,
        // not a generic anyhow error — callers can now match on the variant.
        let client = AetherClient::new("http://localhost:8545");
        let keypair = Keypair::generate();
        let sender_pubkey = PublicKey::from_bytes(keypair.public_key());
        let sender = sender_pubkey.to_address();
        let tx = Transaction {
            nonce: 0,
            chain_id: 1,
            sender,
            sender_pubkey,
            inputs: vec![],
            outputs: vec![],
            reads: HashSet::new(),
            writes: HashSet::new(),
            program_id: None,
            data: vec![],
            gas_limit: 500_000,
            fee: 2_000_000,
            // Deliberately wrong: all-zero bytes, not a valid signature.
            signature: Signature::from_bytes(vec![0; 64]),
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(client.submit(tx)).unwrap_err();
        assert!(
            matches!(err, AetherSdkError::InvalidSignature(_)),
            "expected InvalidSignature, got: {err}"
        );
    }

    #[test]
    fn parse_invalid_endpoint_scheme() {
        let err = HttpEndpoint::parse("https://localhost:8545").unwrap_err();
        assert!(matches!(err, AetherSdkError::InvalidEndpoint(_)));
    }
}
