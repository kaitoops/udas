//! Transport abstraction for IPC.
//!
//! Supports:
//! - `local-socket` feature (default): interprocess local_socket
//!   (Windows named pipe / Unix domain socket)
//! - `tcp` feature: TCP loopback (fallback / debugging)
//!
//! ## Usage
//!
//! ```ignore
//! use udas_embed_service::transport::{create_transport, Transport};
//! use udas_embed_service::config::TransportKind;
//!
//! let t = create_transport(TransportKind::LocalSocket, r"\\.\pipe\udas-embed", "127.0.0.1:9473")?;
//! let listener = t.listen().await?;
//! let stream = t.connect(Duration::from_secs(5)).await?;
//! ```

use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

// ─── DuplexStream ───

/// A bidirectional async stream (read + write).
///
/// Auto-implemented for any type that is `AsyncRead + AsyncWrite + Unpin + Send`.
/// This allows `Box<dyn DuplexStream>` to be used as a type-erased stream,
/// which can then be passed to [`crate::framing::read_frame`] and
/// [`crate::framing::write_frame`].
pub trait DuplexStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> DuplexStream for T {}

/// Type-erased duplex stream.
pub type BoxStream = Box<dyn DuplexStream + Send>;

// ─── Listener ───

/// A listener that accepts incoming connections.
#[async_trait]
pub trait Listener: Send {
    /// Accept the next connection. Blocks until a client connects.
    async fn accept(&mut self) -> anyhow::Result<BoxStream>;
}

/// Type-erased listener.
pub type BoxListener = Box<dyn Listener + Send>;

// ─── Transport trait ───

/// Transport abstraction — creates listeners and connects to servers.
///
/// Implementations:
/// - [`LocalSocketTransport`] (feature `local-socket`) — Windows named pipes
/// - [`TcpTransport`] (feature `tcp`) — TCP loopback
#[async_trait]
pub trait Transport: Send + Sync {
    /// Start listening for incoming connections.
    async fn listen(&self) -> anyhow::Result<BoxListener>;

    /// Connect to a running server with a timeout.
    async fn connect(&self, timeout: Duration) -> anyhow::Result<BoxStream>;

    /// Human-readable address (for logging / PID file).
    fn address_display(&self) -> String;
}

// ─── Local socket implementation ───

#[cfg(feature = "local-socket")]
pub mod local_socket {
    //! Local socket transport using `interprocess` crate.
    //! On Windows this uses named pipes; on Unix, domain sockets.

    use super::*;
    use crate::error::ServiceError;
    use interprocess::local_socket::tokio::prelude::{LocalSocketListener, LocalSocketStream};
    use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
    use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName};
    use tokio::time::timeout;

    /// Transport over interprocess local_socket.
    pub struct LocalSocketTransport {
        name: String,
    }

    impl LocalSocketTransport {
        /// Create with the given socket name.
        ///
        /// On Windows, the bare name is used (e.g. "udas-embed") —
        /// `\\.\pipe\` is prepended automatically by `GenericNamespaced`.
        /// On Unix: abstract namespace (Linux) or `/tmp/` + name (other Unices).
        ///
        /// If the name already starts with `\\.\pipe\`, the prefix is
        /// stripped to avoid double-prefixing.
        pub fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }

        /// Convert the stored name string to an interprocess `Name`.
        ///
        /// Strips the Windows named pipe prefix if present, since
        /// `GenericNamespaced` prepends it automatically.
        fn make_name(&self) -> anyhow::Result<interprocess::local_socket::Name<'_>> {
            let clean = self
                .name
                .strip_prefix(r"\\.\pipe\")
                .unwrap_or(&self.name);
            clean
                .to_ns_name::<GenericNamespaced>()
                .map_err(|e| ServiceError::Transport(format!("invalid name '{}': {e}", self.name)))
                .map_err(Into::into)
        }
    }

    #[async_trait]
    impl Transport for LocalSocketTransport {
        async fn listen(&self) -> anyhow::Result<BoxListener> {
            let name = self.make_name()?;
            let listener = ListenerOptions::new()
                .name(name)
                .create_tokio()
                .map_err(|e| {
                    ServiceError::Transport(format!("bind '{}' failed: {e}", self.name))
                })?;
            tracing::info!(addr = %self.name, "local_socket listener bound");
            Ok(Box::new(LocalSocketListenerWrapper { listener }))
        }

        async fn connect(&self, timeout_dur: Duration) -> anyhow::Result<BoxStream> {
            let name = self.make_name()?;
            let stream = timeout(timeout_dur, LocalSocketStream::connect(name))
                .await
                .map_err(|_| ServiceError::Timeout(timeout_dur))?
                .map_err(|e| ServiceError::ConnectionFailed(e.to_string()))?;
            Ok(Box::new(stream))
        }

        fn address_display(&self) -> String {
            self.name.clone()
        }
    }

    struct LocalSocketListenerWrapper {
        listener: LocalSocketListener,
    }

    #[async_trait]
    impl Listener for LocalSocketListenerWrapper {
        async fn accept(&mut self) -> anyhow::Result<BoxStream> {
            let stream = self
                .listener
                .accept()
                .await
                .map_err(|e| ServiceError::Transport(format!("accept failed: {e}")))?;
            Ok(Box::new(stream))
        }
    }
}

// ─── TCP implementation ───

#[cfg(feature = "tcp")]
pub mod tcp {
    //! TCP loopback transport (fallback / debugging).
    //!
    //! Useful when local_socket is unavailable or for cross-container scenarios.

    use super::*;
    use crate::error::ServiceError;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    /// Transport over TCP loopback.
    pub struct TcpTransport {
        addr: String,
    }

    impl TcpTransport {
        /// Create with the given bind address (e.g. `127.0.0.1:9473`).
        pub fn new(addr: impl Into<String>) -> Self {
            Self { addr: addr.into() }
        }
    }

    #[async_trait]
    impl Transport for TcpTransport {
        async fn listen(&self) -> anyhow::Result<BoxListener> {
            let listener = TcpListener::bind(&self.addr)
                .await
                .map_err(|e| ServiceError::Transport(format!("bind '{}' failed: {e}", self.addr)))?;
            tracing::info!(addr = %self.addr, "tcp listener bound");
            Ok(Box::new(TcpListenerWrapper { listener }))
        }

        async fn connect(&self, timeout_dur: Duration) -> anyhow::Result<BoxStream> {
            let stream = timeout(timeout_dur, TcpStream::connect(&self.addr))
                .await
                .map_err(|_| ServiceError::Timeout(timeout_dur))?
                .map_err(|e| ServiceError::ConnectionFailed(e.to_string()))?;
            Ok(Box::new(stream))
        }

        fn address_display(&self) -> String {
            self.addr.clone()
        }
    }

    struct TcpListenerWrapper {
        listener: TcpListener,
    }

    #[async_trait]
    impl Listener for TcpListenerWrapper {
        async fn accept(&mut self) -> anyhow::Result<BoxStream> {
            let (stream, _peer) = self
                .listener
                .accept()
                .await
                .map_err(|e| ServiceError::Transport(format!("accept failed: {e}")))?;
            Ok(Box::new(stream))
        }
    }
}

// ─── Factory ───

/// Create a transport instance from configuration.
///
/// Returns an error if the requested transport's feature is not enabled.
pub fn create_transport(
    kind: crate::config::TransportKind,
    local_socket_name: &str,
    _tcp_addr: &str,
) -> anyhow::Result<Box<dyn Transport>> {
    match kind {
        crate::config::TransportKind::LocalSocket => {
            #[cfg(feature = "local-socket")]
            {
                Ok(Box::new(local_socket::LocalSocketTransport::new(
                    local_socket_name,
                )))
            }
            #[cfg(not(feature = "local-socket"))]
            {
                Err(anyhow::anyhow!(
                    "local-socket feature not enabled; rebuild with --features local-socket"
                ))
            }
        }
        crate::config::TransportKind::Tcp => {
            #[cfg(feature = "tcp")]
            {
                Ok(Box::new(tcp::TcpTransport::new(tcp_addr)))
            }
            #[cfg(not(feature = "tcp"))]
            {
                Err(anyhow::anyhow!(
                    "tcp feature not enabled; rebuild with --features tcp"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duplex_stream_object_safe() {
        // This test verifies that Box<dyn DuplexStream> compiles.
        // If DuplexStream were not object-safe, this would fail to compile.
        fn _accept_stream(_s: BoxStream) {}
        // (No runtime assertions needed — compilation is the test.)
    }

    #[cfg(feature = "tcp")]
    #[tokio::test]
    async fn test_tcp_echo_roundtrip() {
        // Simple echo test: server reads a frame and writes it back.
        use crate::framing::{read_frame, write_frame};
        use tokio::io::duplex;

        // Use in-memory duplex for unit test (no real TCP needed).
        let (mut client, mut server) = duplex(4096);

        write_frame(&mut client, b"ping").await.unwrap();
        let received = read_frame(&mut server).await.unwrap();
        assert_eq!(received, b"ping");

        write_frame(&mut server, b"pong").await.unwrap();
        let reply = read_frame(&mut client).await.unwrap();
        assert_eq!(reply, b"pong");
    }

    #[cfg(feature = "tcp")]
    #[tokio::test]
    async fn test_tcp_transport_connect_listen() {
        use std::time::Duration;

        // Bind to ephemeral port.
        let server_transport = tcp::TcpTransport::new("127.0.0.1:0");
        // TcpListener::bind with :0 assigns a port, but we need to know it.
        // Use a fixed port unlikely to be in use.
        let addr = "127.0.0.1:19473";
        let server_transport = tcp::TcpTransport::new(addr);

        let mut listener = server_transport.listen().await.unwrap();

        // Spawn acceptor.
        let accept_handle = tokio::spawn(async move {
            let _stream = listener.accept().await.unwrap();
        });

        // Connect from client.
        let client_transport = tcp::TcpTransport::new(addr);
        let _stream = client_transport
            .connect(Duration::from_secs(2))
            .await
            .unwrap();

        // Wait for acceptor to complete.
        accept_handle.await.unwrap();
    }
}
