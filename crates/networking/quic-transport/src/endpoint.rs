use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use quinn::{ClientConfig, Endpoint, ServerConfig, TransportConfig};
use tracing::{debug, info};

use crate::connection::QuicConnection;

/// Production-ready QUIC endpoint with performance tuning
///
/// Optimized for low-latency validator communication:
/// - Aggressive keep-alive (5s)
/// - Large stream/connection windows (10MB)
/// - Fast idle timeout (30s)
/// - Max concurrent streams: 1000
#[derive(Clone)]
pub struct QuicEndpoint {
    inner: Endpoint,
}

impl QuicEndpoint {
    /// Create a new QUIC endpoint bound to the given address
    ///
    /// Uses self-signed certificates for testing/devnet.
    /// Production should use proper PKI certificates.
    pub async fn new(bind_addr: SocketAddr) -> Result<Self> {
        let (cert, key) = generate_self_signed_cert()?;
        Self::new_with_cert(bind_addr, cert, key).await
    }

    /// Create a new QUIC endpoint with a specific certificate
    ///
    /// Used internally and for testing to share certificates between endpoints
    pub async fn new_with_cert(
        bind_addr: SocketAddr,
        cert: rustls::Certificate,
        key: rustls::PrivateKey,
    ) -> Result<Self> {
        let server_config = configure_server(cert.clone(), key)?;
        let mut endpoint =
            Endpoint::server(server_config, bind_addr).context("Failed to bind QUIC endpoint")?;

        let client_config = configure_client(cert)?;
        endpoint.set_default_client_config(client_config);

        info!("QUIC endpoint listening on {}", endpoint.local_addr()?);

        Ok(QuicEndpoint { inner: endpoint })
    }

    /// Get the local address this endpoint is bound to
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.inner
            .local_addr()
            .context("Failed to get local address")
    }

    /// Connect to a remote peer
    pub async fn connect(&self, remote: SocketAddr) -> Result<QuicConnection> {
        debug!("Connecting to {}", remote);

        // Use the actual hostname from the certificate
        let connecting = self
            .inner
            .connect(remote, "validator.aether.local")
            .context("Failed to initiate connection")?;

        let connection = connecting.await.context("Connection handshake failed")?;

        info!("Connected to {}", remote);

        Ok(QuicConnection::new(connection))
    }

    /// Accept an incoming connection
    pub async fn accept(&self) -> Option<QuicConnection> {
        let connecting = self.inner.accept().await?;

        match connecting.await {
            Ok(connection) => {
                info!("Accepted connection from {}", connection.remote_address());
                Some(QuicConnection::new(connection))
            }
            Err(e) => {
                tracing::warn!("Failed to accept connection: {}", e);
                None
            }
        }
    }

    /// Close the endpoint gracefully
    pub fn close(&self) {
        self.inner.close(0u32.into(), b"endpoint shutdown");
    }
}

/// Configure server with production-ready transport settings
fn configure_server(cert: rustls::Certificate, key: rustls::PrivateKey) -> Result<ServerConfig> {
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .context("Failed to configure TLS")?;

    server_crypto.alpn_protocols = vec![b"aether/1".to_vec()];
    server_crypto.max_early_data_size = 0; // Disable 0-RTT for now

    let mut server_config = ServerConfig::with_crypto(Arc::new(server_crypto));
    server_config.transport_config(Arc::new(create_transport_config()));

    Ok(server_config)
}

/// Configure client with production-ready transport settings
fn configure_client(cert: rustls::Certificate) -> Result<ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(&cert)
        .context("Failed to add certificate to root store")?;

    let mut client_crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();

    client_crypto.alpn_protocols = vec![b"aether/1".to_vec()];

    let mut client_config = ClientConfig::new(Arc::new(client_crypto));
    client_config.transport_config(Arc::new(create_transport_config()));

    Ok(client_config)
}

/// Create optimized transport configuration for low-latency validator traffic
///
/// Key optimizations:
/// - 10MB stream/connection windows for high throughput
/// - 5s keep-alive to detect dead connections quickly
/// - 30s idle timeout for fast cleanup
/// - 1000 max concurrent streams for high fan-out (Turbine)
fn create_transport_config() -> TransportConfig {
    let mut config = TransportConfig::default();

    // Large windows for high throughput
    config.max_concurrent_bidi_streams(1000u32.into());
    config.max_concurrent_uni_streams(1000u32.into());
    config.stream_receive_window(10_000_000u32.into()); // 10MB
    config.receive_window(quinn::VarInt::from_u64(10_000_000).unwrap()); // 10MB

    // Aggressive keep-alive for validator liveness
    config.keep_alive_interval(Some(Duration::from_secs(5)));
    config.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));

    // Disable datagram (we use streams for reliability)
    config.datagram_receive_buffer_size(None);

    config
}

/// Generate a self-signed certificate for testing/devnet
///
/// Production validators should use proper PKI certificates
/// with CA-signed certs and certificate pinning.
pub(crate) fn generate_self_signed_cert() -> Result<(rustls::Certificate, rustls::PrivateKey)> {
    let cert = rcgen::generate_simple_self_signed(vec!["validator.aether.local".to_string()])
        .context("Failed to generate certificate")?;

    let key = rustls::PrivateKey(cert.serialize_private_key_der());
    let cert_der = rustls::Certificate(cert.serialize_der().context("Failed to serialize cert")?);

    Ok((cert_der, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_endpoint_creation() {
        let endpoint = QuicEndpoint::new("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        assert!(endpoint.local_addr().is_ok());
    }

    #[tokio::test]
    async fn test_self_signed_cert() {
        let (cert, key) = generate_self_signed_cert().unwrap();
        assert!(!cert.0.is_empty());
        assert!(!key.0.is_empty());
    }

    #[tokio::test]
    async fn test_client_server_connection() {
        // Share the same certificate between server and client for testing
        let (cert, key) = generate_self_signed_cert().unwrap();

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
        tokio::spawn(async move { server_clone.accept().await });

        // Client connects
        let _conn = client.connect(server_addr).await.unwrap();
    }
}
