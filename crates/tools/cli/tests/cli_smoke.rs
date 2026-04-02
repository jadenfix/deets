use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use aether_types::Transaction;
use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn write_config(dir: &tempfile::TempDir, key_path: &str, endpoint: &str) -> String {
    let config_path = dir.path().join("config.toml");
    let contents = format!(
        "endpoint = \"{}\"\ndefault_key = \"{}\"\n",
        endpoint, key_path
    );
    fs::write(&config_path, contents).expect("write config");
    config_path.to_str().unwrap().to_string()
}

struct MockRpcServer {
    addr: SocketAddr,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockRpcServer {
    fn start() -> Option<Self> {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("skipping cli smoke test: cannot bind mock rpc server ({err})");
                return None;
            }
        };
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let addr = listener.local_addr().expect("mock rpc local addr");
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        let handle = thread::spawn(move || {
            while running_clone.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        handle_rpc_connection(&mut stream);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Some(Self {
            addr,
            running,
            handle: Some(handle),
        })
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for MockRpcServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_rpc_connection(stream: &mut TcpStream) {
    let mut req_buf = [0u8; 8192];
    let bytes_read = stream.read(&mut req_buf).unwrap_or(0);
    let request_text = String::from_utf8_lossy(&req_buf[..bytes_read]);
    let tx_hash =
        extract_tx_hash(&request_text).unwrap_or_else(|| format!("0x{}", "ab".repeat(32)));
    let body = format!(r#"{{"jsonrpc":"2.0","result":"{}","id":1}}"#, tx_hash);
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn extract_tx_hash(request_text: &str) -> Option<String> {
    let body = request_text.split("\r\n\r\n").nth(1)?;
    let payload: Value = serde_json::from_str(body).ok()?;
    let tx_hex = payload.get("params")?.get(0)?.as_str()?;
    let tx_bytes = hex::decode(tx_hex.trim_start_matches("0x")).ok()?;
    let tx: Transaction = bincode::deserialize(&tx_bytes).ok()?;
    Some(format!("{:?}", tx.hash()))
}

#[test]
fn transfer_and_job_flow() {
    let temp = TempDir::new().unwrap();
    let Some(rpc_server) = MockRpcServer::start() else {
        return;
    };
    let key_path = temp.path().join("test-key.json");

    let output = Command::cargo_bin("aetherctl")
        .unwrap()
        .args(["keys", "generate", "--out", key_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "key generation failed: {:?}",
        output
    );

    let config = write_config(&temp, key_path.to_str().unwrap(), &rpc_server.endpoint());

    let status = Command::cargo_bin("aetherctl")
        .unwrap()
        .args(["--config", &config, "status"])
        .output()
        .unwrap();
    assert!(status.status.success());

    let recipient = format!("0x{}", "22".repeat(20));
    let transfer = Command::cargo_bin("aetherctl")
        .unwrap()
        .args([
            "--config", &config, "transfer", "--to", &recipient, "--amount", "1000", "--nonce", "1",
        ])
        .output()
        .unwrap();
    assert!(transfer.status.success(), "transfer failed: {:?}", transfer);
    let transfer_json: Value = serde_json::from_slice(&transfer.stdout).unwrap();
    assert_eq!(transfer_json["accepted"].as_bool(), Some(true));
    assert!(transfer_json["tx_hash"].as_str().unwrap().starts_with("0x"));

    let job = Command::cargo_bin("aetherctl")
        .unwrap()
        .args([
            "--config",
            &config,
            "job",
            "post",
            "--job-id",
            "hello-aic-job",
            "--model",
            &format!("0x{}", "12".repeat(32)),
            "--input",
            &format!("0x{}", "ab".repeat(32)),
            "--max-fee",
            "500000000",
            "--expires-at",
            "1700000000",
            "--metadata",
            "{\"prompt\":\"test\"}",
        ])
        .output()
        .unwrap();
    assert!(job.status.success(), "job post failed: {:?}", job);
    let job_json: Value = serde_json::from_slice(&job.stdout).unwrap();
    assert_eq!(
        job_json["payload"]["job_id"].as_str(),
        Some("hello-aic-job")
    );
    assert!(job_json["prepared_matches"].as_bool().unwrap());

    let stake = Command::cargo_bin("aetherctl")
        .unwrap()
        .args([
            "--config", &config, "stake", "delegate", "--amount", "50", "--nonce", "2",
        ])
        .output()
        .unwrap();
    assert!(stake.status.success(), "stake delegate failed: {:?}", stake);
}
