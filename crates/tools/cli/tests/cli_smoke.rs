use std::fs;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn write_config(dir: &tempfile::TempDir, key_path: &str) -> String {
    let config_path = dir.path().join("config.toml");
    let contents = format!(
        "endpoint = \"http://localhost:8545\"\ndefault_key = \"{}\"\n",
        key_path
    );
    fs::write(&config_path, contents).expect("write config");
    config_path.to_str().unwrap().to_string()
}

#[test]
fn transfer_and_job_flow() {
    let temp = TempDir::new().unwrap();
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

    let config = write_config(&temp, key_path.to_str().unwrap());

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
