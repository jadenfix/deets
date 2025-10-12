use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::Result;

type FrameLog = Arc<Mutex<Vec<(SocketAddr, Vec<u8>)>>>;

pub struct QuicConnection {
    remote: SocketAddr,
    frame_log: FrameLog,
}

impl QuicConnection {
    pub(crate) fn new(remote: SocketAddr, frame_log: FrameLog) -> Self {
        QuicConnection { remote, frame_log }
    }

    pub fn remote(&self) -> SocketAddr {
        self.remote
    }

    pub fn send(&self, payload: Vec<u8>) -> Result<()> {
        self.frame_log
            .lock()
            .expect("frame log lock")
            .push((self.remote, payload));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_sent_frames() {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let conn = QuicConnection::new("127.0.0.1:9001".parse().unwrap(), frames.clone());
        conn.send(vec![1, 2, 3]).unwrap();
        assert_eq!(frames.lock().unwrap().len(), 1);
    }
}
