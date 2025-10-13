use aether_types::Transaction;
use anyhow::Result;

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
        tx.calculate_fee()?;
        Ok(SubmitResponse {
            tx_hash: tx.hash(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_types::{Address, H256};
    use serde_json::json;

    #[tokio::test]
    async fn builds_and_submits_transfer() {
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

        let response = client.submit(tx).await.unwrap();
        assert_ne!(response.tx_hash, H256::zero());
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
