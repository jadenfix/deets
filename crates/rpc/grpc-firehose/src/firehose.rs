use anyhow::Result;
use tokio::sync::broadcast;

use aether_types::Block;

use crate::streaming::FirehoseStream;

#[derive(Clone, Debug)]
pub struct FirehoseEvent {
    pub block: Block,
}

pub struct FirehoseServer {
    sender: broadcast::Sender<FirehoseEvent>,
}

impl FirehoseServer {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        FirehoseServer { sender }
    }

    pub fn publish(&self, block: Block) -> Result<()> {
        self.sender
            .send(FirehoseEvent { block })
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!(e))
    }

    pub fn subscribe(&self) -> FirehoseStream {
        FirehoseStream::new(self.sender.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_types::{Address, Block, VrfProof};

    fn empty_block(slot: u64) -> Block {
        Block::new(
            slot,
            aether_types::H256::zero(),
            Address::from_slice(&[0u8; 20]).unwrap(),
            VrfProof {
                output: [0u8; 32],
                proof: Vec::new(),
            },
            Vec::new(),
        )
    }

    #[tokio::test]
    async fn publishes_and_receives() {
        let server = FirehoseServer::new(16);
        let mut stream = server.subscribe();

        server.publish(empty_block(1)).unwrap();
        let event = stream.next().await.unwrap();
        assert_eq!(event.block.header.slot, 1);
    }
}
