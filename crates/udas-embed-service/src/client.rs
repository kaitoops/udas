//! Embedding service client.
//!
//! Connects to a running [`crate::server::EmbedServer`] over IPC and provides:
//! - [`EmbedClient`] — low-level request/response client with typed methods
//! - [`RemoteEmbedder`] — implements `Embedder` trait for drop-in use
//!
//! ## Usage
//!
//! ```ignore
//! use udas_embed_service::{ClientConfig, EmbedClient, RemoteEmbedder};
//! use udas_embedding::Embedder;
//!
//! // Low-level client
//! let client = EmbedClient::connect(ClientConfig::default()).await?;
//! let emb = client.embed("hello world").await?;
//!
//! // As Embedder trait (drop-in replacement)
//! let remote = RemoteEmbedder::connect(ClientConfig::default()).await?;
//! let emb = remote.embed("hello world").await?; // via Embedder trait
//! ```

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;
use tracing::debug;

use udas_embedding::{Embedder, Embedding, UNIFIED_DIM};

use crate::config::ClientConfig;
use crate::error::ServiceError;
use crate::framing::{read_response, write_request};
use crate::protocol::{InfoResult, Request, RequestId, Response, ResponseResult};
use crate::transport::{BoxStream, create_transport};

// ─── EmbedClient ───────────────────────────────────────────────────────

/// Low-level embedding service client.
///
/// Maintains a persistent connection to the server and provides typed
/// methods for each JSON-RPC method. Thread-safe via `Mutex` on the stream
/// — concurrent calls are serialized over a single connection.
///
/// For higher concurrency, create multiple `EmbedClient` instances.
pub struct EmbedClient {
    config: ClientConfig,
    stream: Mutex<BoxStream>,
    next_id: AtomicU64,
}

impl EmbedClient {
    /// Connect to a running server.
    ///
    /// Uses the transport and address from `config`. Fails if the server
    /// is not reachable within `config.connect_timeout`.
    pub async fn connect(config: ClientConfig) -> Result<Self> {
        let transport = create_transport(
            config.transport,
            &config.local_socket_name,
            &config.tcp_addr,
        )?;

        debug!(addr = %transport.address_display(), "EmbedClient: connecting");
        let stream = transport.connect(config.connect_timeout).await?;
        debug!("EmbedClient: connected");

        Ok(Self {
            config,
            stream: Mutex::new(stream),
            next_id: AtomicU64::new(1),
        })
    }

    /// Send an `embed` request (native dimension from server's perspective).
    pub async fn embed(&self, text: &str) -> Result<Vec<f64>> {
        let id = self.next_id();
        let req = Request::embed(id, text);
        let resp = self.send_request(req).await?;
        self.extract_embedding(resp)
    }

    /// Send an `embed_unified` request (projected to UNIFIED_DIM).
    pub async fn embed_unified(&self, text: &str) -> Result<Vec<f64>> {
        let id = self.next_id();
        let req = Request::embed_unified(id, text);
        let resp = self.send_request(req).await?;
        self.extract_embedding(resp)
    }

    /// Send an `embed_batch` request.
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let id = self.next_id();
        let req = Request::embed_batch(id, texts);
        let resp = self.send_request(req).await?;
        self.extract_batch(resp)
    }

    /// Send an `embed_batch_unified` request.
    pub async fn embed_batch_unified(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let id = self.next_id();
        let req = Request::embed_batch_unified(id, texts);
        let resp = self.send_request(req).await?;
        self.extract_batch(resp)
    }

    /// Send a `ping` health check.
    ///
    /// Returns server uptime in seconds.
    pub async fn ping(&self) -> Result<u64> {
        let id = self.next_id();
        let req = Request::ping(id);
        let resp = self.send_request(req).await?;
        match resp.result {
            Some(ResponseResult::Ping { uptime_secs, .. }) => Ok(uptime_secs),
            _ => anyhow::bail!("unexpected response type for ping"),
        }
    }

    /// Send an `info` request.
    pub async fn info(&self) -> Result<InfoResult> {
        let id = self.next_id();
        let req = Request::info(id);
        let resp = self.send_request(req).await?;
        match resp.result {
            Some(ResponseResult::Info(info)) => Ok(info),
            _ => anyhow::bail!("unexpected response type for info"),
        }
    }

    /// Get the client configuration (for diagnostics).
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    // ─── Internal helpers ───

    fn next_id(&self) -> RequestId {
        self.next_id.fetch_add(1, Ordering::Relaxed).into()
    }

    async fn send_request(&self, req: Request) -> Result<Response> {
        let mut stream = self.stream.lock().await;

        // Write request
        write_request(&mut *stream, &req).await?;

        // Read response with timeout
        let resp = tokio::time::timeout(self.config.request_timeout, read_response(&mut *stream))
            .await
            .map_err(|_| ServiceError::Timeout(self.config.request_timeout))??;

        // Check for RPC error
        if resp.is_error() {
            if let Some(ref err) = resp.error {
                anyhow::bail!("server error [{}]: {}", err.code, err.message);
            }
        }

        Ok(resp)
    }

    fn extract_embedding(&self, resp: Response) -> Result<Vec<f64>> {
        match resp.result {
            Some(ResponseResult::Embed { embedding }) => Ok(embedding),
            _ => anyhow::bail!("expected Embed result, got something else"),
        }
    }

    fn extract_batch(&self, resp: Response) -> Result<Vec<Vec<f64>>> {
        match resp.result {
            Some(ResponseResult::EmbedBatch { embeddings }) => Ok(embeddings),
            _ => anyhow::bail!("expected EmbedBatch result, got something else"),
        }
    }
}

// ─── RemoteEmbedder ────────────────────────────────────────────────────

/// Remote embedder — implements `Embedder` trait via IPC.
///
/// Drop-in replacement for local embedders (BGE-M3, FNV, etc.) that
/// delegates to a running `EmbedServer`. Useful when the model loading
/// cost is too high to pay per invocation (10+ min for BGE-M3 cold start).
///
/// Created with [`RemoteEmbedder::connect`] or [`RemoteEmbedder::with_client`].
#[derive(Clone)]
pub struct RemoteEmbedder {
    client: Arc<EmbedClient>,
}

impl RemoteEmbedder {
    /// Create a new `RemoteEmbedder` wrapping an existing client.
    pub fn with_client(client: EmbedClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// Connect to a server and create a `RemoteEmbedder`.
    pub async fn connect(config: ClientConfig) -> Result<Self> {
        let client = EmbedClient::connect(config).await?;
        Ok(Self::with_client(client))
    }

    /// Access the underlying client (for ping/info diagnostics).
    pub fn client(&self) -> &EmbedClient {
        &self.client
    }
}

#[async_trait]
impl Embedder for RemoteEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        self.client.embed(text).await
    }

    fn native_dim(&self) -> usize {
        // The server always returns UNIFIED_DIM (512) after projection
        UNIFIED_DIM
    }

    fn name(&self) -> &str {
        "remote"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_remote_embedder_name() {
        // RemoteEmbedder::name() is synchronous, just check it returns "remote"
        // We can't easily test connect without a running server,
        // but we can verify the trait method exists.
        fn assert_embedder<T: Embedder>() {}
        assert_embedder::<RemoteEmbedder>();
    }

    #[tokio::test]
    async fn test_client_connect_failure() {
        // Connecting to a non-existent server should fail
        let mut config = ClientConfig::default();
        config.connect_timeout = Duration::from_millis(100);
        // Use a name that won't have a server listening
        config.local_socket_name = "udas-embed-test-nonexistent".to_string();

        let result = EmbedClient::connect(config).await;
        assert!(
            result.is_err(),
            "Should fail to connect to non-existent server"
        );
    }

    #[tokio::test]
    async fn test_remote_embedder_connect_failure() {
        let mut config = ClientConfig::default();
        config.connect_timeout = Duration::from_millis(100);
        config.local_socket_name = "udas-embed-test-nonexistent-2".to_string();

        let result = RemoteEmbedder::connect(config).await;
        assert!(result.is_err());
    }
}
