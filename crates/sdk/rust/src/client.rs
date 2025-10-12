use aether_types::Transaction;
use anyhow::Result;

use crate::transaction_builder::TransferBuilder;
use crate::types::{ClientConfig, SubmitResponse};

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

    pub fn transfer(&self) -> TransferBuilder<'_> {
        TransferBuilder::new(&self.config)
    }

    pub async fn submit(&self, tx: Transaction) -> Result<SubmitResponse> {
        tx.verify_signature()?;
        tx.calculate_fee()?;
        Ok(SubmitResponse {
            tx_hash: tx.hash(),
            accepted: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use aether_types::{Address, H256};

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
}
