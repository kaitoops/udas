//! udas-embedding — Embedding layer for UDAS.
//!
//! Provides the `Embedder` trait and implementations:
//! - `FnvHashEmbedder` — deterministic hash-based (zero-dependency, always available)
//! - `BgeM3Embedder` — BGE-M3 ONNX with CUDA (requires `ort-backend` feature)
//! - `BgeSmallEmbedder` — BGE-small ONNX CPU fallback (requires `ort-backend` feature)
//!
//! Also provides `DimensionProjector` for unifying embedding dimensions
//! to the UDAS standard (512), and `RuntimeEmbedder` for automatic backend
//! selection.
//!
//! ## Architecture
//!
//! ```text
//! Embedder (trait)
//! ├── FnvHashEmbedder       256-d, zero-dependency, always available
//! ├── BgeM3Embedder         1024-d → 512-d projected, GPU (ort-backend)
//! ├── BgeSmallEmbedder      512-d native, CPU fallback (ort-backend)
//! └── RuntimeEmbedder       auto-selects best available + projects to 512-d
//!
//! DimensionProjector       unifies any dimension → 512-d
//! CachedEmbedder           disk + memory cache wrapper
//! ```

// Always available modules
pub mod embedder;
pub mod fnv;
pub mod projector;
pub mod gpu;

// Feature-gated modules (require ort + tokenizers)
#[cfg(feature = "ort-backend")]
pub mod pooling;
#[cfg(feature = "ort-backend")]
pub mod bge_m3;
#[cfg(feature = "ort-backend")]
pub mod bge_small;

// Cache module (requires sha2 for hash, tempfile for tests)
pub mod cache;

// Runtime selection (always available, gracefully degrades)
pub mod runtime;

pub use embedder::{Embedder, Embedding, UNIFIED_DIM};
pub use fnv::FnvHashEmbedder;
pub use projector::DimensionProjector;
pub use gpu::GpuState;
pub use cache::CachedEmbedder;
pub use runtime::RuntimeEmbedder;
