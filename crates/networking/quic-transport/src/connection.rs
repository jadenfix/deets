use std::net::SocketAddr;

use anyhow::{Context, Result};
use bytes::Bytes;
use quinn::{Connection, RecvStream, SendStream};
use tracing::debug;

/// QUIC connection wrapper with streaming API
///
/// Provides send/receive primitives for validator communication.
/// Uses unidirectional streams for one-way messages (most common)
/// and bidirectional streams for request/response patterns.
pub struct QuicConnection {
    inner: Connection,
}

impl QuicConnection {
    pub(crate) fn new(connection: Connection) -> Self {
        QuicConnection { inner: connection }
    }

    /// Get the remote address of this connection
    pub fn remote(&self) -> SocketAddr {
        self.inner.remote_address()
    }

    /// Send a message on a unidirectional stream
    ///
    /// Opens a new stream, writes the data, and closes it.
    /// This is the most efficient pattern for one-way messages
    /// like block propagation, vote broadcasts, etc.
    pub async fn send(&self, data: impl Into<Bytes>) -> Result<()> {
        let mut stream = self
            .inner
            .open_uni()
            .await
            .context("Failed to open uni stream")?;

        let data = data.into();
        stream
            .write_all(&data)
            .await
            .context("Failed to write to stream")?;

        stream.finish().await.context("Failed to finish stream")?;

        debug!("Sent {} bytes to {}", data.len(), self.remote());

        Ok(())
    }

    /// Send a message on a bidirectional stream and await a response
    ///
    /// Useful for RPC-style request/response patterns like
    /// repair requests, state sync queries, etc.
    pub async fn send_request(&self, data: impl Into<Bytes>) -> Result<Vec<u8>> {
        let (mut send, mut recv) = self
            .inner
            .open_bi()
            .await
            .context("Failed to open bi stream")?;

        // Send request
        let data = data.into();
        send.write_all(&data)
            .await
            .context("Failed to write request")?;
        send.finish().await.context("Failed to finish send")?;

        // Receive response
        let response = recv
            .read_to_end(10_000_000) // 10MB max response
            .await
            .context("Failed to read response")?;

        debug!(
            "Sent {} bytes, received {} bytes from {}",
            data.len(),
            response.len(),
            self.remote()
        );

        Ok(response)
    }

    /// Accept an incoming unidirectional stream
    pub async fn accept_uni(&self) -> Result<RecvStream> {
        self.inner
            .accept_uni()
            .await
            .context("Failed to accept uni stream")
    }

    /// Accept an incoming bidirectional stream
    pub async fn accept_bi(&self) -> Result<(SendStream, RecvStream)> {
        self.inner
            .accept_bi()
            .await
            .context("Failed to accept bi stream")
    }

    /// Read all data from a stream (up to 10MB)
    pub async fn read_stream(stream: &mut RecvStream) -> Result<Vec<u8>> {
        stream
            .read_to_end(10_000_000)
            .await
            .context("Failed to read stream")
    }

    /// Close the connection gracefully
    pub fn close(&self, reason: &str) {
        self.inner.close(0u32.into(), reason.as_bytes());
    }

    /// Get connection statistics for monitoring
    pub fn stats(&self) -> ConnectionStats {
        let stats = self.inner.stats();
        // Note: Quinn doesn't expose total bytes, only UDP datagrams
        // For now, use 0 as placeholder until we add our own tracking
        ConnectionStats {
            bytes_sent: 0,
            bytes_received: 0,
            rtt: stats.path.rtt,
        }
    }
}

/// Connection statistics for monitoring
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub rtt: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::QuicEndpoint;

    #[tokio::test]
    async fn test_unidirectional_send() {
        // Helper to create test endpoints with shared cert
        let (cert, key) = crate::endpoint::generate_self_signed_cert().unwrap();

        let server =
            QuicEndpoint::new_with_cert("127.0.0.1:0".parse().unwrap(), cert.clone(), key.clone())
                .await
                .unwrap();
        let server_addr = server.local_addr().unwrap();

        let client = QuicEndpoint::new_with_cert("127.0.0.1:0".parse().unwrap(), cert, key)
            .await
            .unwrap();

        // Spawn server accept task
        let server_clone = server.clone();
        tokio::spawn(async move {
            if let Some(conn) = server_clone.accept().await {
                let mut stream = conn.accept_uni().await.unwrap();
                let data = QuicConnection::read_stream(&mut stream).await.unwrap();
                assert_eq!(data, b"hello");
            }
        });

        // Client sends message
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let conn = client.connect(server_addr).await.unwrap();
        conn.send(Bytes::from_static(b"hello")).await.unwrap();
    }

    #[tokio::test]
    async fn test_bidirectional_request() {
        let (cert, key) = crate::endpoint::generate_self_signed_cert().unwrap();

        let server =
            QuicEndpoint::new_with_cert("127.0.0.1:0".parse().unwrap(), cert.clone(), key.clone())
                .await
                .unwrap();
        let server_addr = server.local_addr().unwrap();

        let client = QuicEndpoint::new_with_cert("127.0.0.1:0".parse().unwrap(), cert, key)
            .await
            .unwrap();

        // Spawn server task that echoes back
        let server_clone = server.clone();
        tokio::spawn(async move {
            if let Some(conn) = server_clone.accept().await {
                let (mut send, mut recv) = conn.accept_bi().await.unwrap();
                let data = QuicConnection::read_stream(&mut recv).await.unwrap();
                send.write_all(&data).await.unwrap();
                send.finish().await.unwrap();
            }
        });

        // Client sends request and receives response
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let conn = client.connect(server_addr).await.unwrap();
        let response = conn
            .send_request(Bytes::from_static(b"ping"))
            .await
            .unwrap();
        assert_eq!(response, b"ping");
    }

    #[tokio::test]
    async fn test_connection_stats() {
        let (cert, key) = crate::endpoint::generate_self_signed_cert().unwrap();

        let server =
            QuicEndpoint::new_with_cert("127.0.0.1:0".parse().unwrap(), cert.clone(), key.clone())
                .await
                .unwrap();
        let server_addr = server.local_addr().unwrap();

        let client = QuicEndpoint::new_with_cert("127.0.0.1:0".parse().unwrap(), cert, key)
            .await
            .unwrap();

        tokio::spawn(async move { server.accept().await });

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let conn = client.connect(server_addr).await.unwrap();

        let stats = conn.stats();
        // Stats should be available after connection
        // Just verify we can get stats without panicking
        let _ = stats.rtt;
    }
}
