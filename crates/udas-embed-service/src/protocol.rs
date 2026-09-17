//! JSON-RPC 2.0 protocol types for the embedding service.
//!
//! All messages are transported as length-delimited frames (see [`crate::framing`]).
//! Each frame contains a UTF-8 JSON payload matching one of the types below.

use serde::{Deserialize, Serialize};

// ─── Constants ───

/// JSON-RPC protocol version string.
pub const JSONRPC_VERSION: &str = "2.0";

/// Maximum frame payload size: 32 MB.
/// Single embedding (1024-d f64) ~= 8 KB; batch of 1000 ~= 8 MB.
pub const MAX_FRAME_SIZE: usize = 32 * 1024 * 1024;

// ─── Method names ───

/// Supported JSON-RPC method names.
pub mod method {
    /// Single text embedding (native dimension).
    pub const EMBED: &str = "embed";
    /// Batch text embedding (native dimension).
    pub const EMBED_BATCH: &str = "embed_batch";
    /// Single text embedding, projected to UNIFIED_DIM (512).
    pub const EMBED_UNIFIED: &str = "embed_unified";
    /// Batch text embedding, projected to UNIFIED_DIM (512).
    pub const EMBED_BATCH_UNIFIED: &str = "embed_batch_unified";
    /// Health check — lightweight, no inference.
    pub const PING: &str = "ping";
    /// Server info — backend, dimensions, uptime, request count.
    pub const INFO: &str = "info";
}

// ─── Error codes (JSON-RPC 2.0 standard + custom) ───

/// JSON-RPC 2.0 error codes.
pub mod error_code {
    /// JSON parse error.
    pub const PARSE_ERROR: i32 = -32700;
    /// Valid JSON but not a valid request object.
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found or not supported.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameters.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal server error.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Backend (model inference) error.
    pub const BACKEND_ERROR: i32 = -32000;
    /// Server is shutting down.
    pub const SERVER_SHUTTING_DOWN: i32 = -32001;
}

// ─── Request ID ───

/// JSON-RPC request identifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric ID (most common).
    Int(i64),
    /// String ID (for client-generated correlation).
    Str(String),
}

impl Default for RequestId {
    fn default() -> Self {
        RequestId::Int(0)
    }
}

impl From<i64> for RequestId {
    fn from(v: i64) -> Self {
        RequestId::Int(v)
    }
}

impl From<u64> for RequestId {
    fn from(v: u64) -> Self {
        RequestId::Int(v as i64)
    }
}

impl From<i32> for RequestId {
    fn from(v: i32) -> Self {
        RequestId::Int(v as i64)
    }
}

impl From<&str> for RequestId {
    fn from(v: &str) -> Self {
        RequestId::Str(v.to_string())
    }
}

// ─── Request ───

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    /// Parameters as raw JSON; parsed into typed structs by the dispatcher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl Request {
    /// Create a new request with the given method and params.
    pub fn new(
        id: impl Into<RequestId>,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            method: method.to_string(),
            params,
        }
    }

    /// Create an `embed` request (native dimension).
    pub fn embed(id: impl Into<RequestId>, text: &str) -> Self {
        Self::new(id, method::EMBED, Some(serde_json::json!({ "text": text })))
    }

    /// Create an `embed_batch` request (native dimension).
    pub fn embed_batch(id: impl Into<RequestId>, texts: &[String]) -> Self {
        Self::new(
            id,
            method::EMBED_BATCH,
            Some(serde_json::json!({ "texts": texts })),
        )
    }

    /// Create an `embed_unified` request (projected to 512-d).
    pub fn embed_unified(id: impl Into<RequestId>, text: &str) -> Self {
        Self::new(
            id,
            method::EMBED_UNIFIED,
            Some(serde_json::json!({ "text": text })),
        )
    }

    /// Create an `embed_batch_unified` request (projected to 512-d).
    pub fn embed_batch_unified(id: impl Into<RequestId>, texts: &[String]) -> Self {
        Self::new(
            id,
            method::EMBED_BATCH_UNIFIED,
            Some(serde_json::json!({ "texts": texts })),
        )
    }

    /// Create a `ping` request (health check).
    pub fn ping(id: impl Into<RequestId>) -> Self {
        Self::new(id, method::PING, None)
    }

    /// Create an `info` request (server metadata).
    pub fn info(id: impl Into<RequestId>) -> Self {
        Self::new(id, method::INFO, None)
    }
}

// ─── Typed params (for dispatch) ───

/// Parameters for `embed` / `embed_unified`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedParams {
    pub text: String,
}

/// Parameters for `embed_batch` / `embed_batch_unified`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedBatchParams {
    pub texts: Vec<String>,
}

// ─── Response ───

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: RequestId,
    /// Present on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResponseResult>,
    /// Present on error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// Create a success response.
    pub fn success(id: RequestId, result: ResponseResult) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(id: RequestId, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Create an error response with附加 data.
    pub fn error_with_data(
        id: RequestId,
        code: i32,
        message: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
        }
    }

    /// Check if this is an error response.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Result payload — varies by method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseResult {
    /// Single embedding (`embed` / `embed_unified`).
    Embed { embedding: Vec<f64> },
    /// Batch embeddings (`embed_batch` / `embed_batch_unified`).
    EmbedBatch { embeddings: Vec<Vec<f64>> },
    /// Ping response.
    Ping { pong: bool, uptime_secs: u64 },
    /// Info response.
    Info(InfoResult),
}

/// Server info returned by the `info` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoResult {
    /// Backend name (e.g. "bge-m3-cuda", "bge-small-cpu", "fnv-hash").
    pub backend: String,
    /// Native embedding dimension before projection.
    pub native_dim: usize,
    /// Output dimension (after projection if applicable).
    pub output_dim: usize,
    /// Model directory path.
    pub model_dir: String,
    /// Server uptime in seconds.
    pub uptime_secs: u64,
    /// Total requests served.
    pub requests_served: u64,
}

// ─── RPC Error ───

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ─── Serialization helpers ───

/// Serialize a value to JSON bytes.
pub fn to_json_bytes<T: Serialize>(value: &T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(value)
}

/// Deserialize JSON bytes.
pub fn from_json_bytes<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> serde_json::Result<T> {
    serde_json::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialize_embed() {
        let req = Request::embed(1, "hello world");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"method\":\"embed\""));
        assert!(json.contains("\"text\":\"hello world\""));
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn test_request_deserialize() {
        let json = r#"{"jsonrpc":"2.0","id":42,"method":"embed","params":{"text":"test"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "embed");
        assert_eq!(req.id, RequestId::Int(42));
        assert!(req.params.is_some());
    }

    #[test]
    fn test_response_success_roundtrip() {
        let resp = Response::success(
            RequestId::Int(1),
            ResponseResult::Embed {
                embedding: vec![0.1, 0.2, 0.3],
            },
        );
        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, RequestId::Int(1));
        assert!(back.result.is_some());
        assert!(back.error.is_none());
    }

    #[test]
    fn test_response_error() {
        let resp = Response::error(
            RequestId::Int(1),
            error_code::METHOD_NOT_FOUND,
            "unknown method",
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"code\":-32601"));
        assert!(json.contains("\"error\""));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn test_ping_request_no_params() {
        let req = Request::ping(1);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"method\":\"ping\""));
        assert!(!json.contains("params"));
    }

    #[test]
    fn test_batch_response() {
        let resp = Response::success(
            RequestId::Int(2),
            ResponseResult::EmbedBatch {
                embeddings: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
            },
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"embeddings\""));
    }

    #[test]
    fn test_request_id_string() {
        let json = r#"{"jsonrpc":"2.0","id":"abc-123","method":"ping"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, RequestId::Str("abc-123".to_string()));
    }

    #[test]
    fn test_embed_params_parse() {
        let params = serde_json::json!({ "text": "hello" });
        let typed: EmbedParams = serde_json::from_value(params).unwrap();
        assert_eq!(typed.text, "hello");
    }

    #[test]
    fn test_embed_batch_params_parse() {
        let params = serde_json::json!({ "texts": ["a", "b", "c"] });
        let typed: EmbedBatchParams = serde_json::from_value(params).unwrap();
        assert_eq!(typed.texts.len(), 3);
    }

    #[test]
    fn test_info_result_serialize() {
        let info = InfoResult {
            backend: "bge-m3-cuda".to_string(),
            native_dim: 1024,
            output_dim: 512,
            model_dir: "/models/bge-m3".to_string(),
            uptime_secs: 3600,
            requests_served: 12345,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: InfoResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend, "bge-m3-cuda");
        assert_eq!(back.native_dim, 1024);
    }

    #[test]
    fn test_batch_request() {
        let texts = vec!["hello".to_string(), "world".to_string()];
        let req = Request::embed_batch(1, &texts);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"embed_batch\""));
        assert!(json.contains("\"texts\""));
    }
}
