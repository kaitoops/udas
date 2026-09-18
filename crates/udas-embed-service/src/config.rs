//! Configuration for the embedding service.

use std::path::PathBuf;
use std::time::Duration;

/// Default local socket name (Windows named pipe path).
pub const DEFAULT_LOCAL_SOCKET_NAME: &str = "udas-embed";

/// Default TCP bind address.
pub const DEFAULT_TCP_ADDR: &str = "127.0.0.1:9473";

/// Default model directory (matches udas-embedding DEFAULT_MODEL_DIR).
pub const DEFAULT_MODEL_DIR: &str = "C:\\Users\\WIN10\\udas-tui\\models";

// ─── Transport kind ───

/// Transport type selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportKind {
    /// interprocess local_socket (Windows named pipe / Unix domain socket).
    #[default]
    LocalSocket,
    /// TCP loopback (fallback / debugging).
    Tcp,
}

impl std::fmt::Display for TransportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportKind::LocalSocket => write!(f, "local_socket"),
            TransportKind::Tcp => write!(f, "tcp"),
        }
    }
}

impl std::str::FromStr for TransportKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local_socket" | "localsocket" | "socket" => Ok(TransportKind::LocalSocket),
            "tcp" => Ok(TransportKind::Tcp),
            other => Err(format!("unknown transport: {other}")),
        }
    }
}

// ─── Backend mode ───

/// Backend selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendMode {
    /// Auto-select: GPU BGE-M3 -> CPU BGE-small -> FNV.
    #[default]
    Auto,
    /// Force BGE-M3 (GPU).
    BgeM3,
    /// Force BGE-small (CPU).
    BgeSmall,
    /// Force FNV hash (no model needed, zero-dependency).
    Fnv,
}

impl std::fmt::Display for BackendMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendMode::Auto => write!(f, "auto"),
            BackendMode::BgeM3 => write!(f, "bge-m3"),
            BackendMode::BgeSmall => write!(f, "bge-small"),
            BackendMode::Fnv => write!(f, "fnv"),
        }
    }
}

impl std::str::FromStr for BackendMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(BackendMode::Auto),
            "bge-m3" | "bgem3" | "m3" => Ok(BackendMode::BgeM3),
            "bge-small" | "bgesmall" | "small" => Ok(BackendMode::BgeSmall),
            "fnv" | "hash" => Ok(BackendMode::Fnv),
            other => Err(format!("unknown backend: {other}")),
        }
    }
}

// ─── Server config ───

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub transport: TransportKind,
    pub local_socket_name: String,
    pub tcp_addr: String,
    pub model_dir: PathBuf,
    pub backend: BackendMode,
    pub prefer_gpu: bool,
    pub max_connections: usize,
    pub request_timeout: Duration,
    pub shutdown_grace: Duration,
    pub log_dir: PathBuf,
    pub pid_file: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            transport: TransportKind::default(),
            local_socket_name: DEFAULT_LOCAL_SOCKET_NAME.to_string(),
            tcp_addr: DEFAULT_TCP_ADDR.to_string(),
            model_dir: PathBuf::from(DEFAULT_MODEL_DIR),
            backend: BackendMode::default(),
            prefer_gpu: true,
            max_connections: 64,
            request_timeout: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(30),
            log_dir: home.join(".udas").join("logs"),
            pid_file: home.join(".udas").join("embed-service.pid"),
        }
    }
}

// ─── Client config ───

/// Client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub transport: TransportKind,
    pub local_socket_name: String,
    pub tcp_addr: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub retry_max: u32,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            transport: TransportKind::default(),
            local_socket_name: DEFAULT_LOCAL_SOCKET_NAME.to_string(),
            tcp_addr: DEFAULT_TCP_ADDR.to_string(),
            connect_timeout: Duration::from_millis(500),
            request_timeout: Duration::from_secs(5),
            retry_max: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_kind_from_str() {
        assert_eq!(
            "local_socket".parse::<TransportKind>().unwrap(),
            TransportKind::LocalSocket
        );
        assert_eq!("tcp".parse::<TransportKind>().unwrap(), TransportKind::Tcp);
        assert!("invalid".parse::<TransportKind>().is_err());
    }

    #[test]
    fn test_backend_mode_from_str() {
        assert_eq!("auto".parse::<BackendMode>().unwrap(), BackendMode::Auto);
        assert_eq!("bge-m3".parse::<BackendMode>().unwrap(), BackendMode::BgeM3);
        assert_eq!("fnv".parse::<BackendMode>().unwrap(), BackendMode::Fnv);
    }

    #[test]
    fn test_server_config_default() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.transport, TransportKind::LocalSocket);
        assert_eq!(cfg.max_connections, 64);
        assert!(cfg.prefer_gpu);
    }

    #[test]
    fn test_client_config_default() {
        let cfg = ClientConfig::default();
        assert_eq!(cfg.connect_timeout, Duration::from_millis(500));
        assert_eq!(cfg.retry_max, 2);
    }
}
