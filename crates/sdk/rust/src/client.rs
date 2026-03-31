use aether_types::Transaction;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::job_builder::JobBuilder;
use crate::transaction_builder::TransferBuilder;
use crate::types::{ClientConfig, JobRequest, JobSubmission, SubmitResponse};

#[derive(Clone, Debug)]
pub struct AetherClient {
    endpoint: String,
    config: ClientConfig,
}

impl AetherClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        AetherClient {
            endpoint: endpoint.into(),
            config: ClientConfig::default(),
        }
    }

    pub fn with_config(endpoint: impl Into<String>, config: ClientConfig) -> Self {
        AetherClient {
            endpoint: endpoint.into(),
            config,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub fn transfer(&self) -> TransferBuilder {
        TransferBuilder::new(&self.config)
    }

    pub fn job(&self) -> JobBuilder {
        JobBuilder::new(&self.endpoint)
    }

    pub async fn submit(&self, tx: Transaction) -> Result<SubmitResponse> {
        tx.verify_signature()?;
        let fee_params = aether_types::ChainConfig::devnet().fees;
        tx.calculate_fee(&fee_params)?;

        let tx_hash = tx.hash();
        let payload = SubmitRpcRequest::new(tx)?;
        let endpoint = HttpEndpoint::parse(&self.endpoint)?;
        let body = serde_json::to_vec(&payload).context("failed to encode rpc request body")?;
        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            endpoint.path,
            endpoint.host_header(),
            body.len()
        );

        let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
            .await
            .with_context(|| format!("failed to submit transaction to {}", self.endpoint))?;
        stream
            .write_all(request.as_bytes())
            .await
            .context("failed to write rpc request headers")?;
        stream
            .write_all(&body)
            .await
            .context("failed to write rpc request body")?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .context("failed to read rpc response")?;
        let response_text = String::from_utf8(raw).context("rpc response was not valid utf-8")?;
        let (status_line, rpc_body) = parse_http_response(&response_text)?;
        if !status_line.contains(" 200 ") {
            return Err(anyhow!("rpc returned non-success status: {status_line}"));
        }

        let response: JsonRpcResponse<String> =
            serde_json::from_str(rpc_body).context("failed to decode rpc response body")?;

        if let Some(error) = response.error {
            return Err(anyhow!("rpc error {}: {}", error.code, error.message));
        }

        let result_hash = response
            .result
            .ok_or_else(|| anyhow!("rpc response missing result"))?;
        let returned_hash = parse_h256_hex(&result_hash)?;

        if returned_hash != tx_hash {
            return Err(anyhow!(
                "rpc returned mismatched tx hash: expected {:?}, got {:?}",
                tx_hash,
                returned_hash
            ));
        }

        Ok(SubmitResponse {
            tx_hash: returned_hash,
            accepted: true,
        })
    }

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
    fn new(tx: Transaction) -> Result<Self> {
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

struct HttpEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl HttpEndpoint {
    fn parse(endpoint: &str) -> Result<Self> {
        let trimmed = endpoint.trim();
        let without_scheme = trimmed
            .strip_prefix("http://")
            .ok_or_else(|| anyhow!("only http:// endpoints are supported, got: {trimmed}"))?;

        let (host_port, path) = if let Some((h, p)) = without_scheme.split_once('/') {
            (h, format!("/{}", p))
        } else {
            (without_scheme, "/".to_string())
        };

        if host_port.is_empty() {
            return Err(anyhow!("invalid endpoint host: {endpoint}"));
        }

        let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
            let parsed_port = p
                .parse::<u16>()
                .with_context(|| format!("invalid endpoint port in {endpoint}"))?;
            (h.to_string(), parsed_port)
        } else {
            (host_port.to_string(), 80)
        };

        if host.is_empty() {
            return Err(anyhow!("invalid endpoint host: {endpoint}"));
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

fn parse_http_response(response: &str) -> Result<(&str, &str)> {
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("invalid http response from rpc endpoint"))?;
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| anyhow!("missing http status line"))?;
    Ok((status_line, body))
}

fn parse_h256_hex(value: &str) -> Result<aether_types::H256> {
    let bytes = hex::decode(value.trim_start_matches("0x"))
        .with_context(|| format!("invalid tx hash hex: {value}"))?;
    aether_types::H256::from_slice(&bytes).map_err(|e| anyhow!("invalid tx hash: {e}"))
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
        assert!(err.to_string().contains("failed to submit transaction"));
    }

    #[test]
    fn builds_job_submission_payload() {
        let client = AetherClient::new("http://localhost:8545");
        let model_hash = H256::from_slice(&[1u8; 32]).unwrap();
        let input_hash = H256::from_slice(&[2u8; 32]).unwrap();

        let submission = client
            .job()
            .job_id("hello-aic-job")
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
}
