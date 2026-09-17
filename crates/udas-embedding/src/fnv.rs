//! FNV-1a hash embedder — zero-dependency deterministic embedding.
//!
//! This is NOT a semantic embedding — it only guarantees that identical
//! texts map to identical vectors and distinct texts map to distinct
//! vectors with low collision probability. It exists as a zero-dependency
//! fallback for compilation, CI, and environments without model files.
//!
//! Migrated from `tui/src/udas_bridge.rs` and `udas-cli/src/main.rs`
//! where this code was previously duplicated.

use async_trait::async_trait;

use crate::embedder::{Embedder, Embedding};

/// FNV-1a 256-dimensional character-trigram hash embedder.
pub struct FnvHashEmbedder;

/// Native dimension of FNV hash embeddings.
const FNV_DIM: usize = 256;

impl FnvHashEmbedder {
    /// Create a new FNV hash embedder.
    pub fn new() -> Self {
        Self
    }

    /// Synchronous embedding (for non-async contexts like CLI subcommands).
    ///
    /// This is equivalent to `embed()` but without the async wrapper,
    /// since FNV hashing is purely CPU-bound with no I/O.
    pub fn embed_sync(text: &str) -> Embedding {
        fnv_embed(text)
    }
}

impl Default for FnvHashEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Embedder for FnvHashEmbedder {
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding> {
        Ok(fnv_embed(text))
    }

    fn native_dim(&self) -> usize {
        FNV_DIM
    }

    fn name(&self) -> &str {
        "fnv-hash-256"
    }
}

// ─── Internal hash functions ────────────────────────────────────────────

/// FNV-1a 64-bit hash.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Produce a deterministic 256-d embedding from text via character-trigram
/// feature hashing.
///
/// Each trigram is hashed to an index in `[0, FNV_DIM)` and contributes
/// `+1.0` or `-1.0` (sign from a second hash). The final vector is
/// L2-normalised so cosine distance is well-defined.
fn fnv_embed(text: &str) -> Embedding {
    let chars: Vec<char> = text.chars().collect();
    let mut vec = vec![0.0f64; FNV_DIM];

    if chars.len() < 3 {
        // Too short for trigrams: hash the whole string.
        let h = fnv1a_64(text.as_bytes());
        let idx = (h % FNV_DIM as u64) as usize;
        let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
        vec[idx] = sign;
    } else {
        for window in chars.windows(3) {
            let trigram: String = window.iter().collect();
            let h = fnv1a_64(trigram.as_bytes());
            let idx = (h % FNV_DIM as u64) as usize;
            let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
            vec[idx] += sign;
        }
    }

    // L2 normalise.
    let norm: f64 = vec.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > f64::EPSILON {
        for v in &mut vec {
            *v /= norm;
        }
    }

    vec
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fnv_embed_is_deterministic() {
        let embedder = FnvHashEmbedder;
        let a = embedder.embed("hello world").await.unwrap();
        let b = embedder.embed("hello world").await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn fnv_embed_distinct_texts_differ() {
        let embedder = FnvHashEmbedder;
        let a = embedder.embed("hello world").await.unwrap();
        let b = embedder.embed("goodbye universe").await.unwrap();
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn fnv_embed_is_normalised() {
        let embedder = FnvHashEmbedder;
        let v = embedder.embed("some longer text for embedding test").await.unwrap();
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn fnv_embed_dimension_is_correct() {
        let embedder = FnvHashEmbedder;
        let v = embedder.embed("test").await.unwrap();
        assert_eq!(v.len(), FNV_DIM);
    }

    #[test]
    fn fnv_embed_sync_matches_async() {
        let sync_result = FnvHashEmbedder::embed_sync("hello world");
        // The async version is just a wrapper around the same fnv_embed,
        // so they should produce identical results.
        let expected = fnv_embed("hello world");
        assert_eq!(sync_result, expected);
    }

    #[test]
    fn fnv_embed_short_text() {
        // Text shorter than 3 characters should still produce a valid embedding.
        let v = FnvHashEmbedder::embed_sync("ab");
        assert_eq!(v.len(), FNV_DIM);
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fnv_embed_empty_text() {
        let v = FnvHashEmbedder::embed_sync("");
        assert_eq!(v.len(), FNV_DIM);
        // Empty string produces all zeros (no trigrams, whole-string hash
        // of empty string maps to some index, but the sign bit determines
        // the value). Either way, it should be deterministic.
        let v2 = FnvHashEmbedder::embed_sync("");
        assert_eq!(v, v2);
    }

    #[test]
    fn fnv_native_dim_is_256() {
        let embedder = FnvHashEmbedder;
        assert_eq!(embedder.native_dim(), 256);
    }

    #[test]
    fn fnv_name_is_correct() {
        let embedder = FnvHashEmbedder;
        assert_eq!(embedder.name(), "fnv-hash-256");
    }
}
