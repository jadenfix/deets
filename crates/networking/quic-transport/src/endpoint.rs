use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::connection::QuicConnection;

type FrameLog = Arc<Mutex<Vec<(SocketAddr, Vec<u8>)>>>;

#[derive(Clone)]
pub struct QuicEndpoint {
    local_addr: SocketAddr,
    frame_log: FrameLog,
}

impl QuicEndpoint {
    pub fn new(local_addr: SocketAddr) -> Self {
        QuicEndpoint {
            local_addr,
            frame_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn connect(&self, remote: SocketAddr) -> Result<QuicConnection> {
        Ok(QuicConnection::new(remote, self.frame_log.clone()))
    }

    pub fn sent_frames(&self) -> Vec<(SocketAddr, Vec<u8>)> {
        self.frame_log.lock().expect("frame log lock").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connects_and_logs_frames() {
        let endpoint = QuicEndpoint::new("127.0.0.1:7000".parse().unwrap());
        let conn = endpoint.connect("127.0.0.1:8000".parse().unwrap()).unwrap();
        conn.send(vec![1, 2, 3]).unwrap();
        assert_eq!(endpoint.sent_frames().len(), 1);
    }
}
