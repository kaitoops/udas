//! Embedding service server.
//!
//! Loads the embedding model once at startup and serves embedding requests
//! over IPC (local_socket / TCP). Each client connection is handled in its
//! own tokio task.
//!
//! ## Lifecycle
//!
//! 1. [`EmbedServer::new`] — loads model (may take 10+ min for BGE-M3 cold start)
//! 2. [`EmbedServer::run`] — binds listener and accepts connections
//! 3. [`EmbedServer::shutdown`] — signals graceful shutdown
//!
//! ## Request Flow
//!
//! ```text
//! Client connects
//!   -> read_request (JSON-RPC frame)
//!   -> dispatch (match method)
//!   -> embedder.embed(text)
//!   -> write_response (JSON-RPC frame)
//!   -> loop until client disconnects
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::Result;
use tokio::sync::{Notify, Semaphore};
use tracing::{error, info, warn};

use udas_embedding::{Embedder, RuntimeEmbedder};

use crate::config::ServerConfig;
use crate::framing::{read_request, write_response};
use crate::protocol::{
    EmbedBatchParams, EmbedParams, InfoResult, Request, Response, ResponseResult, error_code,
    method,
};
use crate::transport::{BoxStream, create_transport};

/// Embedding service server.
///
/// Holds a loaded [`RuntimeEmbedder`] and serves requests over IPC.
/// The model is loaded once at construction time; subsequent requests
/// only pay inference cost (<1s for BGE-M3, <10ms for FNV).
pub struct EmbedServer {
    config: ServerConfig,
    embedder: Arc<RuntimeEmbedder>,
    start_time: Instant,
    requests_served: Arc<AtomicU64>,
    shutdown: Arc<Notify>,
    connection_sem: Arc<Semaphore>,
}

impl EmbedServer {
    /// Create a new server with the given configuration.
    ///
    /// This loads the embedding model synchronously. For BGE-M3 with GPU,
    /// this may take 10+ minutes (cold start). The tradeoff is that all
    /// subsequent requests are fast (<1s).
    pub fn new(config: ServerConfig) -> Result<Self> {
        info!(
            backend = ?config.backend,
            model_dir = %config.model_dir.display(),
            transport = %config.transport,
            "EmbedServer: initializing"
        );

        let embedder = RuntimeEmbedder::with_model_dir(&config.model_dir);
        info!(
            backend = %embedder.backend_name(),
            native_dim = embedder.native_dim(),
            output_dim = embedder.output_dim(),
            "EmbedServer: model loaded"
        );

        let max_conn = config.max_connections;
        Ok(Self {
            embedder: Arc::new(embedder),
            config,
            start_time: Instant::now(),
            requests_served: Arc::new(AtomicU64::new(0)),
            shutdown: Arc::new(Notify::new()),
            connection_sem: Arc::new(Semaphore::new(max_conn)),
        })
    }

    /// Run the server until shutdown is signaled.
    ///
    /// Main event loop: accept connections, spawn a task per connection,
    /// and wait for the shutdown signal.
    pub async fn run(&self) -> Result<()> {
        let transport = create_transport(
            self.config.transport,
            &self.config.local_socket_name,
            &self.config.tcp_addr,
        )?;

        let addr = transport.address_display();
        info!(addr = %addr, "EmbedServer: binding listener");

        self.write_pid_file()?;

        let mut listener = transport.listen().await?;
        info!("EmbedServer: ready, accepting connections");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(stream) => {
                            let embedder = self.embedder.clone();
                            let requests_served = self.requests_served.clone();
                            let start_time = self.start_time;
                            let sem = self.connection_sem.clone();
                            let model_dir = self.config.model_dir.display().to_string();

                            tokio::spawn(async move {
                                let _permit = match sem.acquire_owned().await {
                                    Ok(p) => p,
                                    Err(_) => {
                                        warn!("Connection semaphore closed");
                                        return;
                                    }
                                };
                                handle_connection(
                                    stream,
                                    embedder,
                                    requests_served,
                                    start_time,
                                    model_dir,
                                )
                                .await;
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "Accept failed, retrying in 100ms");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    }
                }
                _ = self.shutdown.notified() => {
                    info!("EmbedServer: shutdown signal received");
                    break;
                }
            }
        }

        self.remove_pid_file();
        info!("EmbedServer: stopped");
        Ok(())
    }

    /// Signal the server to shut down gracefully.
    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    /// Get current server info (for the `info` RPC method and diagnostics).
    pub fn info(&self) -> InfoResult {
        InfoResult {
            backend: self.embedder.backend_name().to_string(),
            native_dim: self.embedder.native_dim(),
            output_dim: self.embedder.output_dim(),
            model_dir: self.config.model_dir.display().to_string(),
            uptime_secs: self.start_time.elapsed().as_secs(),
            requests_served: self.requests_served.load(Ordering::Relaxed),
        }
    }

    fn write_pid_file(&self) -> Result<()> {
        if let Some(parent) = self.config.pid_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.config.pid_file, std::process::id().to_string())?;
        info!(pid_file = %self.config.pid_file.display(), "PID file written");
        Ok(())
    }

    fn remove_pid_file(&self) {
        if self.config.pid_file.exists()
            && let Err(e) = std::fs::remove_file(&self.config.pid_file)
        {
            warn!(error = %e, "Failed to remove PID file");
        }
    }
}

/// Handle a single client connection.
///
/// Reads requests in a loop, dispatches to the embedder, and writes responses.
/// Exits when the client disconnects or an unrecoverable error occurs.
async fn handle_connection(
    mut stream: BoxStream,
    embedder: Arc<RuntimeEmbedder>,
    requests_served: Arc<AtomicU64>,
    start_time: Instant,
    model_dir: String,
) {
    loop {
        let request = match read_request(&mut stream).await {
            Ok(req) => req,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Client disconnected cleanly
                break;
            }
            Err(e) => {
                warn!(error = %e, "Failed to read request, closing connection");
                break;
            }
        };

        let response = dispatch(
            &request,
            &embedder,
            &requests_served,
            start_time,
            &model_dir,
        )
        .await;

        if let Err(e) = write_response(&mut stream, &response).await {
            warn!(error = %e, "Failed to write response, closing connection");
            break;
        }
    }
}

/// Dispatch a JSON-RPC request to the appropriate handler.
async fn dispatch(
    request: &Request,
    embedder: &RuntimeEmbedder,
    requests_served: &AtomicU64,
    start_time: Instant,
    model_dir: &str,
) -> Response {
    let id = request.id.clone();
    let uptime = start_time.elapsed().as_secs();

    let result: anyhow::Result<ResponseResult> = match request.method.as_str() {
        method::EMBED | method::EMBED_UNIFIED => handle_embed(request, embedder).await,
        method::EMBED_BATCH | method::EMBED_BATCH_UNIFIED => {
            handle_embed_batch(request, embedder).await
        }
        method::PING => Ok(ResponseResult::Ping {
            pong: true,
            uptime_secs: uptime,
        }),
        method::INFO => Ok(ResponseResult::Info(InfoResult {
            backend: embedder.backend_name().to_string(),
            native_dim: embedder.native_dim(),
            output_dim: embedder.output_dim(),
            model_dir: model_dir.to_string(),
            uptime_secs: uptime,
            requests_served: requests_served.load(Ordering::Relaxed),
        })),
        _ => {
            return Response::error(
                id,
                error_code::METHOD_NOT_FOUND,
                format!("unknown method: {}", request.method),
            );
        }
    };

    requests_served.fetch_add(1, Ordering::Relaxed);

    match result {
        Ok(response_result) => Response::success(id, response_result),
        Err(e) => {
            warn!(error = %e, method = %request.method, "Request failed");
            Response::error(id, error_code::BACKEND_ERROR, e.to_string())
        }
    }
}

/// Handle `embed` / `embed_unified` requests.
async fn handle_embed(
    request: &Request,
    embedder: &RuntimeEmbedder,
) -> anyhow::Result<ResponseResult> {
    let params: EmbedParams = request
        .params
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing params for embed"))
        .and_then(|p| {
            serde_json::from_value(p.clone())
                .map_err(|e| anyhow::anyhow!("invalid embed params: {e}"))
        })?;

    let embedding = embedder.embed(&params.text).await?;
    Ok(ResponseResult::Embed { embedding })
}

/// Handle `embed_batch` / `embed_batch_unified` requests.
async fn handle_embed_batch(
    request: &Request,
    embedder: &RuntimeEmbedder,
) -> anyhow::Result<ResponseResult> {
    let params: EmbedBatchParams = request
        .params
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing params for embed_batch"))
        .and_then(|p| {
            serde_json::from_value(p.clone())
                .map_err(|e| anyhow::anyhow!("invalid embed_batch params: {e}"))
        })?;

    let mut embeddings = Vec::with_capacity(params.texts.len());
    for text in &params.texts {
        let emb = embedder.embed(text).await?;
        embeddings.push(emb);
    }
    Ok(ResponseResult::EmbedBatch { embeddings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;
    use crate::protocol::ResponseResult;

    #[test]
    fn test_server_creation() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config);
        assert!(server.is_ok());
        let server = server.unwrap();
        // With default features (no ort-backend), should fall back to FNV
        assert!(server.embedder.backend_name().contains("fnv"));
    }

    #[test]
    fn test_server_info() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let info = server.info();

        assert_eq!(info.requests_served, 0);
        assert!(info.backend.contains("fnv"));
    }

    #[tokio::test]
    async fn test_dispatch_ping() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let requests_served = Arc::new(AtomicU64::new(0));

        let req = Request::ping(1);
        let resp = dispatch(
            &req,
            &server.embedder,
            &requests_served,
            server.start_time,
            "",
        )
        .await;

        assert!(!resp.is_error());
        match resp.result {
            Some(ResponseResult::Ping { pong, .. }) => assert!(pong),
            _ => panic!("expected Ping result"),
        }
    }

    #[tokio::test]
    async fn test_dispatch_embed() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let requests_served = Arc::new(AtomicU64::new(0));

        let req = Request::embed(1, "hello world");
        let resp = dispatch(
            &req,
            &server.embedder,
            &requests_served,
            server.start_time,
            "",
        )
        .await;

        assert!(!resp.is_error());
        match resp.result {
            Some(ResponseResult::Embed { embedding }) => {
                assert!(!embedding.is_empty());
            }
            _ => panic!("expected Embed result"),
        }
    }

    #[tokio::test]
    async fn test_dispatch_unknown_method() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let requests_served = Arc::new(AtomicU64::new(0));

        let req = Request::new(1, "nonexistent", None);
        let resp = dispatch(
            &req,
            &server.embedder,
            &requests_served,
            server.start_time,
            "",
        )
        .await;

        assert!(resp.is_error());
        assert_eq!(resp.error.unwrap().code, error_code::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dispatch_embed_batch() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let requests_served = Arc::new(AtomicU64::new(0));

        let texts = vec!["hello".to_string(), "world".to_string()];
        let req = Request::embed_batch(1, &texts);
        let resp = dispatch(
            &req,
            &server.embedder,
            &requests_served,
            server.start_time,
            "",
        )
        .await;

        assert!(!resp.is_error());
        match resp.result {
            Some(ResponseResult::EmbedBatch { embeddings }) => {
                assert_eq!(embeddings.len(), 2);
            }
            _ => panic!("expected EmbedBatch result"),
        }
    }

    #[tokio::test]
    async fn test_dispatch_info() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let requests_served = Arc::new(AtomicU64::new(0));

        let req = Request::info(1);
        let resp = dispatch(
            &req,
            &server.embedder,
            &requests_served,
            server.start_time,
            "/test/models",
        )
        .await;

        assert!(!resp.is_error());
        match resp.result {
            Some(ResponseResult::Info(info)) => {
                assert_eq!(info.model_dir, "/test/models");
                assert!(info.backend.contains("fnv"));
            }
            _ => panic!("expected Info result"),
        }
    }

    #[tokio::test]
    async fn test_requests_served_counter() {
        let config = ServerConfig::default();
        let server = EmbedServer::new(config).unwrap();
        let requests_served = Arc::new(AtomicU64::new(0));

        for i in 0..3 {
            let req = Request::ping(i);
            dispatch(
                &req,
                &server.embedder,
                &requests_served,
                server.start_time,
                "",
            )
            .await;
        }

        assert_eq!(requests_served.load(Ordering::Relaxed), 3);
    }
}
