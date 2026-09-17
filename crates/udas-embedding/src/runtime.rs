//! Runtime embedder selection — picks the best available embedder.
//!
//! Selection logic:
//! 1. Check GPU state → if available, use BGE-M3 (GPU)
//! 2. If GPU busy, use BGE-small (CPU)
//! 3. If models not found, fall back to FNV hash
//!
//! All selected embedders are wrapped in CachedEmbedder for disk caching.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cache::CachedEmbedder;
use crate::embedder::{Embedder, UNIFIED_DIM};
use crate::fnv::FnvHashEmbedder;
use crate::gpu::GpuState;
use crate::projector::DimensionProjector;

/// Default model directory.
const DEFAULT_MODEL_DIR: &str = r"C:\Users\WIN10\udas-tui\models";

/// Default cache directory.
fn default_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".udas")
        .join("embedding_cache")
}

/// Runtime embedder that dynamically selects backend and projects to unified dim.
pub struct RuntimeEmbedder {
    inner: Box<dyn Embedder>,
    projector: DimensionProjector,
    backend_name: String,
}

impl RuntimeEmbedder {
    /// Create the best available embedder at runtime.
    ///
    /// Selection order:
    /// 1. BGE-M3 GPU (if model exists and GPU free)
    /// 2. BGE-small CPU (if model exists)
    /// 3. FNV hash (zero-dependency fallback)
    pub fn auto() -> Self {
        Self::with_model_dir(Path::new(DEFAULT_MODEL_DIR))
    }

    /// Create with explicit model directory.
    pub fn with_model_dir(model_dir: &Path) -> Self {
        let cache_dir = default_cache_dir();
        let gpu_state = GpuState::detect();

        tracing::info!("RuntimeEmbedder: GPU state: {}", gpu_state.description());

        // Try BGE-M3 GPU
        if gpu_state.is_available() {
            let m3_dir = model_dir.join("bge-m3-onnx");
            if m3_dir.join("model.onnx").exists() {
                match BgeM3EmbedderWrapper::new(&m3_dir) {
                    Ok(e) => {
                        let native_dim = e.native_dim();
                        tracing::info!("RuntimeEmbedder: using BGE-M3 GPU ({native_dim}-d)");
                        return Self::wrap(
                            Box::new(CachedEmbedder::new(e, cache_dir)),
                            native_dim,
                            "bge-m3".to_string(),
                        );
                    }
                    Err(e) => {
                        tracing::warn!("BGE-M3 init failed: {e}, trying fallback");
                    }
                }
            } else {
                tracing::info!(
                    "BGE-M3 model not found at {}, skipping GPU path",
                    m3_dir.display()
                );
            }
        }

        // Try BGE-small CPU
        let small_dir = model_dir.join("bge-small-zh-onnx");
        if small_dir.join("model.onnx").exists() {
            match BgeSmallEmbedderWrapper::new(&small_dir) {
                Ok(e) => {
                    let native_dim = e.native_dim();
                    tracing::info!("RuntimeEmbedder: using BGE-small CPU ({native_dim}-d)");
                    return Self::wrap(
                        Box::new(CachedEmbedder::new(e, cache_dir)),
                        native_dim,
                        "bge-small".to_string(),
                    );
                }
                Err(e) => {
                    tracing::warn!("BGE-small init failed: {e}, using FNV fallback");
                }
            }
        } else {
            tracing::info!(
                "BGE-small model not found at {}, skipping CPU path",
                small_dir.display()
            );
        }

        // FNV hash fallback
        tracing::info!("RuntimeEmbedder: using FNV hash (256-d, no semantic embedding)");
        let fnv = FnvHashEmbedder;
        Self::wrap(Box::new(fnv), 256, "fnv-hash".to_string())
    }

    fn wrap(inner: Box<dyn Embedder>, native_dim: usize, name: String) -> Self {
        Self {
            inner,
            projector: DimensionProjector::to_unified(native_dim),
            backend_name: name,
        }
    }

    /// Get the backend name (for diagnostics).
    pub fn backend_name(&self) -> &str {
        &self.backend_name
    }

    /// Get the native dimension before projection.
    pub fn native_dim(&self) -> usize {
        self.inner.native_dim()
    }

    /// Get the output (unified) dimension.
    pub fn output_dim(&self) -> usize {
        UNIFIED_DIM
    }
}

#[async_trait::async_trait]
impl Embedder for RuntimeEmbedder {
    async fn embed(&self, text: &str) -> Result<crate::embedder::Embedding> {
        let native = self.inner.embed(text).await?;
        Ok(self.projector.project(&native))
    }

    fn native_dim(&self) -> usize {
        UNIFIED_DIM // After projection, always UNIFIED_DIM
    }

    fn name(&self) -> &str {
        &self.backend_name
    }
}

// ─── Wrapper types for conditional compilation ──────────────────────────
//
// When ort/tokenizers are not available (Phase 1, no model files),
// these wrappers fall back to FNV. When ort IS available, they delegate
// to the real BGE embedders.

#[cfg(feature = "ort-backend")]
mod ort_backends {
    pub use crate::bge_m3::BgeM3EmbedderAsync as BgeM3Wrapper;
    pub use crate::bge_small::BgeSmallEmbedderAsync as BgeSmallWrapper;
}

#[cfg(not(feature = "ort-backend"))]
mod fnv_backends {
    use crate::embedder::{Embedder, Embedding};
    use crate::fnv::FnvHashEmbedder;
    use anyhow::Result;
    use async_trait::async_trait;

    /// FNV-based fallback when ort feature is not enabled.
    pub struct BgeM3Wrapper(FnvHashEmbedder);
    impl BgeM3Wrapper {
        pub fn new(_model_dir: &std::path::Path) -> anyhow::Result<Self> {
            anyhow::bail!("ort-backend feature not enabled — BGE-M3 unavailable")
        }
    }
    #[async_trait]
    impl Embedder for BgeM3Wrapper {
        async fn embed(&self, text: &str) -> Result<Embedding> {
            self.0.embed(text).await
        }
        fn native_dim(&self) -> usize {
            1024
        }
        fn name(&self) -> &str {
            "bge-m3-unavailable"
        }
    }

    pub struct BgeSmallWrapper(FnvHashEmbedder);
    impl BgeSmallWrapper {
        pub fn new(_model_dir: &std::path::Path) -> anyhow::Result<Self> {
            anyhow::bail!("ort-backend feature not enabled — BGE-small unavailable")
        }
    }
    #[async_trait]
    impl Embedder for BgeSmallWrapper {
        async fn embed(&self, text: &str) -> Result<Embedding> {
            self.0.embed(text).await
        }
        fn native_dim(&self) -> usize {
            512
        }
        fn name(&self) -> &str {
            "bge-small-unavailable"
        }
    }
}

#[cfg(not(feature = "ort-backend"))]
use fnv_backends::{
    BgeM3Wrapper as BgeM3EmbedderWrapper, BgeSmallWrapper as BgeSmallEmbedderWrapper,
};
#[cfg(feature = "ort-backend")]
use ort_backends::{
    BgeM3Wrapper as BgeM3EmbedderWrapper, BgeSmallWrapper as BgeSmallEmbedderWrapper,
};
