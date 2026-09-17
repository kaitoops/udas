//! udas-embed-service — Persistent embedding service for UDAS.
//!
//! Provides an IPC server that loads the embedding model once and serves
//! requests over local_socket (Windows named pipes) or TCP loopback.
//!
//! ## Architecture
//!
//! ```text
//! Client (CLI/TUI/engine)                     Server (udas-embed-server)
//! +--------------------+                     +--------------------------+
//! | RemoteEmbedder     | --- IPC --------->  | EmbedServer              |
//! |  (impl Embedder)   | <-- IPC ---------  |  +- RuntimeEmbedder      |
//! |                    |                     |  |   (BGE-M3 / BGE-small  |
//! | EmbedClient        |                     |  |    / FNV)              |
//! |  (connect + retry) |                     |  +- Transport (listener) |
//! +--------------------+                     |  +- Request dispatcher    |
//!                                          +--------------------------+
//! ```
//!
//! ## Protocol
//!
//! JSON-RPC 2.0 over length-delimited frames (4B big-endian length + JSON).
//! See [`protocol`] for message types and [`framing`] for codec.

pub mod client;
pub mod config;
pub mod error;
pub mod framing;
pub mod protocol;
pub mod server;
pub mod transport;

// Re-exports — protocol types
pub use protocol::{
    EmbedBatchParams, EmbedParams, InfoResult, Request, RequestId, Response, ResponseResult,
    RpcError,
};

// Re-exports — config types
pub use config::{BackendMode, ClientConfig, ServerConfig, TransportKind};

// Re-exports — error types
pub use error::ServiceError;

// Re-exports — client types
pub use client::{EmbedClient, RemoteEmbedder};

// Re-exports — server types
pub use server::EmbedServer;

// Re-exports — transport types
pub use transport::{create_transport, BoxListener, BoxStream, DuplexStream, Listener, Transport};
