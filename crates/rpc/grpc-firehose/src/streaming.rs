use tokio::sync::broadcast::{error::RecvError, Receiver};

use crate::firehose::FirehoseEvent;

pub struct FirehoseStream {
    inner: Receiver<FirehoseEvent>,
}

impl FirehoseStream {
    pub fn new(inner: Receiver<FirehoseEvent>) -> Self {
        FirehoseStream { inner }
    }

    pub async fn next(&mut self) -> Option<FirehoseEvent> {
        loop {
            match self.inner.recv().await {
                Ok(event) => return Some(event),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    }
}
