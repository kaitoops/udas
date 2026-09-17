//! Embedder trait — the core abstraction for text embedding in UDAS.
//!
//! This trait is intentionally separate from `LlmRestorer` to allow
//! swapping embedding backends (FNV hash → BGE-M3 → BGE-small) without
//! touching the LLM restoration pipeline.

use async_trait::async_trait;

/// A semantic embedding vector.
pub type Embedding = Vec<f64>;

/// Unified embedding dimension for all UDAS interference field computations.
///
/// Chosen as 512 based on:
/// - BGE-M3 (1024-d) projects down via JL lemma with minimal information loss
/// - BGE-small (512-d) is native — no projection needed
/// - FNV hash (256-d) zero-pads — interface consistency only
/// - Interference field O(N²) computation benefits from smaller dimension
pub const UNIFIED_DIM: usize = 512;

/// Trait for text embedding — independent of LLM restoration.
///
/// Implementations:
/// - `FnvHashEmbedder` — deterministic hash-based, zero-dependency (Phase 1)
/// - `BgeM3Embedder` — BGE-M3 ONNX, GPU (Phase 2)
/// - `BgeSmallEmbedder` — BGE-small ONNX, CPU fallback (Phase 3)
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Generate an embedding vector for the given text.
    ///
    /// The returned vector has `native_dim()` elements. Callers that need
    /// the unified dimension should pass the result through
    /// `DimensionProjector`.
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding>;

    /// The native output dimension of this embedder (before projection).
    fn native_dim(&self) -> usize;

    /// Embedder name for diagnostics and logging.
    fn name(&self) -> &str;
}
