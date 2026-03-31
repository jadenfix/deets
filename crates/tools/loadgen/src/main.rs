use aether_crypto_primitives::Keypair;
use aether_types::{Address, PublicKey, Signature, Transaction};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Copy)]
enum WorkloadProfile {
    Transfer,
    Mixed,
}

impl WorkloadProfile {
    fn from_str(value: &str) -> Self {
        match value {
            "mixed" => Self::Mixed,
            _ => Self::Transfer,
        }
    }
}

struct HttpEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl HttpEndpoint {
    fn parse(endpoint: &str) -> Result<Self> {
        let without_scheme = endpoint
            .trim()
            .strip_prefix("http://")
            .ok_or_else(|| anyhow!("only http:// endpoints are supported: {endpoint}"))?;

        let (host_port, path) = if let Some((host, rest)) = without_scheme.split_once('/') {
            (host, format!("/{}", rest))
        } else {
            (without_scheme, "/".to_string())
        };

        if host_port.is_empty() {
            return Err(anyhow!("invalid endpoint host: {endpoint}"));
        }

        let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
            let parsed = p
                .parse::<u16>()
                .with_context(|| format!("invalid endpoint port in {endpoint}"))?;
            (h.to_string(), parsed)
        } else {
            (host_port.to_string(), 80)
        };

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

/// Generate a signed synthetic transfer transaction accepted by mempool validation.
fn generate_transfer(sender_key: &Keypair, recipient: Address, nonce: u64) -> Result<Transaction> {
    let sender_pubkey = PublicKey::from_bytes(sender_key.public_key());
    let sender = sender_pubkey.to_address();

    let mut reads = HashSet::new();
    reads.insert(sender);

    let mut writes = HashSet::new();
    writes.insert(sender);
    writes.insert(recipient);

    let mut tx = Transaction {
        nonce,
        chain_id: 1,
        sender,
        sender_pubkey,
        inputs: vec![],
        outputs: vec![],
        reads,
        writes,
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee: 500_000,
        signature: Signature::from_bytes(vec![]),
    };

    let hash = tx.hash();
    tx.signature = Signature::from_bytes(sender_key.sign(hash.as_bytes()));
    Ok(tx)
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

async fn submit_tx(endpoint: &HttpEndpoint, tx: &Transaction) -> Result<bool> {
    let tx_bytes = bincode::serialize(tx).context("failed to serialize transaction")?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "aeth_sendRawTransaction",
        "params": [format!("0x{}", hex::encode(tx_bytes))],
        "id": 1,
    })
    .to_string();

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        endpoint.path,
        endpoint.host_header(),
        body.len(),
        body
    );

    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .await
        .with_context(|| format!("failed to connect to {}:{}", endpoint.host, endpoint.port))?;
    stream
        .write_all(request.as_bytes())
        .await
        .context("failed to write rpc request")?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .context("failed to read rpc response")?;
    let response_text =
        String::from_utf8(response).context("rpc response was not valid utf-8 bytes")?;
    let status_line = response_text
        .lines()
        .next()
        .ok_or_else(|| anyhow!("missing http status line"))?;
    if !status_line.contains(" 200 ") {
        return Ok(false);
    }

    let (_, body) = parse_http_response(&response_text)?;
    let rpc: JsonRpcResponse<String> =
        serde_json::from_str(body).context("failed to decode rpc response")?;
    if let Some(err) = rpc.error {
        eprintln!("rpc rejected tx: {} {}", err.code, err.message);
        return Ok(false);
    }

    Ok(rpc.result.is_some())
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

struct RunStats {
    total_sent: u64,
    total_ok: u64,
    total_err: u64,
    elapsed: Duration,
}

impl RunStats {
    fn print(&self) {
        let tps = if self.elapsed.as_secs_f64() > 0.0 {
            self.total_sent as f64 / self.elapsed.as_secs_f64()
        } else {
            0.0
        };

        println!("\n--- Load Generator Results ---");
        println!("Duration:   {:.1}s", self.elapsed.as_secs_f64());
        println!("Sent:       {}", self.total_sent);
        println!("Succeeded:  {}", self.total_ok);
        println!("Failed:     {}", self.total_err);
        println!("Throughput: {:.1} tx/s", tps);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Aether Load Generator v0.1.0");
    println!("============================\n");

    let rpc_url =
        std::env::var("LOADGEN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
    let endpoint = HttpEndpoint::parse(&rpc_url)?;
    let target_tps: u64 = std::env::var("LOADGEN_TPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let duration_secs: u64 = std::env::var("LOADGEN_DURATION")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let profile = WorkloadProfile::from_str(
        &std::env::var("LOADGEN_PROFILE").unwrap_or_else(|_| "transfer".to_string()),
    );

    println!("RPC:      {rpc_url}");
    println!("Target:   {target_tps} tx/s");
    println!("Duration: {duration_secs}s");
    println!("Profile:  {profile:?}");
    println!();

    let sender_key = Keypair::generate();
    let recipient = PublicKey::from_bytes(Keypair::generate().public_key()).to_address();
    println!(
        "Sender:   0x{}",
        hex::encode(
            PublicKey::from_bytes(sender_key.public_key())
                .to_address()
                .as_bytes()
        )
    );
    println!("Recipient: 0x{}", hex::encode(recipient.as_bytes()));
    println!();

    let interval = Duration::from_micros(1_000_000 / target_tps.max(1));
    let deadline = Instant::now() + Duration::from_secs(duration_secs);

    let mut nonce: u64 = 0;
    let mut ok: u64 = 0;
    let mut err: u64 = 0;
    let start = Instant::now();

    while Instant::now() < deadline {
        let tx = match profile {
            WorkloadProfile::Transfer | WorkloadProfile::Mixed => {
                generate_transfer(&sender_key, recipient, nonce)?
            }
        };

        match submit_tx(&endpoint, &tx).await {
            Ok(true) => ok += 1,
            Ok(false) | Err(_) => err += 1,
        }

        nonce += 1;
        tokio::time::sleep(interval).await;
    }

    let stats = RunStats {
        total_sent: nonce,
        total_ok: ok,
        total_err: err,
        elapsed: start.elapsed(),
    };
    stats.print();

    Ok(())
}
