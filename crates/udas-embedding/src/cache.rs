//! Disk-cached embedder wrapper — avoids redundant embedding computations.
//!
//! Wraps any `Embedder` and caches results to disk using SHA-256(text) as key.
//! Cache format: binary file with [4 bytes u32 dim][dim * 8 bytes f64 values].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::embedder::{Embedder, Embedding};

/// Disk + memory cached embedder wrapper.
pub struct CachedEmbedder<E: Embedder + 'static> {
    inner: Arc<E>,
    cache_dir: PathBuf,
    memory_cache: RwLock<HashMap<[u8; 32], Embedding>>,
}

impl<E: Embedder + 'static> CachedEmbedder<E> {
    /// Create a new cached embedder.
    ///
    /// # Arguments
    /// * `inner` - The underlying embedder to cache results for
    /// * `cache_dir` - Directory for disk cache files (e.g., `~/.udas/embedding_cache/`)
    pub fn new(inner: E, cache_dir: PathBuf) -> Self {
        // Ensure cache directory exists
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create cache dir {}: {e}", cache_dir.display());
        }

        // Load existing cache index into memory
        let mut initial_cache = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if name.len() != 64 {
                    // SHA-256 hex = 64 chars
                    continue;
                }
                let Ok(hash) = hex_to_bytes(&name) else {
                    continue;
                };
                let Ok(emb) = read_embedding(&entry.path()) else {
                    continue;
                };
                initial_cache.insert(hash, emb);
            }
        }
        tracing::info!(
            "CachedEmbedder: loaded {} entries from {}",
            initial_cache.len(),
            cache_dir.display()
        );

        Self {
            inner: Arc::new(inner),
            cache_dir,
            memory_cache: RwLock::new(initial_cache),
        }
    }

    /// Get cache statistics.
    pub async fn cache_size(&self) -> usize {
        self.memory_cache.read().await.len()
    }

    /// Clear all cached entries.
    pub async fn clear_cache(&self) -> Result<()> {
        self.memory_cache.write().await.clear();
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir)?;
            std::fs::create_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }
}

#[async_trait]
impl<E: Embedder + 'static> Embedder for CachedEmbedder<E> {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let hash = sha256_bytes(text.as_bytes());

        // Check memory cache
        {
            let cache = self.memory_cache.read().await;
            if let Some(emb) = cache.get(&hash) {
                return Ok(emb.clone());
            }
        }

        // Check disk cache
        let hex = bytes_to_hex(&hash);
        let cache_file = self.cache_dir.join(&hex);
        if cache_file.exists()
            && let Ok(emb) = read_embedding(&cache_file)
        {
            self.memory_cache.write().await.insert(hash, emb.clone());
            return Ok(emb);
        }

        // Compute embedding
        let emb = self.inner.embed(text).await?;

        // Write to disk
        if let Err(e) = write_embedding(&cache_file, &emb) {
            tracing::warn!("Failed to write cache entry {}: {e}", cache_file.display());
        }

        // Write to memory
        self.memory_cache.write().await.insert(hash, emb.clone());

        Ok(emb)
    }

    fn native_dim(&self) -> usize {
        self.inner.native_dim()
    }

    fn name(&self) -> &str {
        "cached"
    }
}

// ─── Binary cache format ────────────────────────────────────────────────

/// Write an embedding to a binary cache file.
///
/// Format: [4 bytes: u32 dim (little-endian)] [dim * 8 bytes: f64 values (little-endian)]
fn write_embedding(path: &Path, embedding: &[f64]) -> Result<()> {
    let dim = embedding.len() as u32;
    let mut data = Vec::with_capacity(4 + embedding.len() * 8);

    data.extend_from_slice(&dim.to_le_bytes());
    for &v in embedding {
        data.extend_from_slice(&v.to_le_bytes());
    }

    std::fs::write(path, &data)?;
    Ok(())
}

/// Read an embedding from a binary cache file.
fn read_embedding(path: &Path) -> Result<Embedding> {
    let data = std::fs::read(path)?;
    if data.len() < 4 {
        anyhow::bail!("Cache file too short: {}", path.display());
    }

    let dim = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    let expected_len = 4 + dim * 8;
    if data.len() < expected_len {
        anyhow::bail!(
            "Cache file truncated: expected {expected_len} bytes, got {}",
            data.len()
        );
    }

    let mut embedding = Vec::with_capacity(dim);
    for i in 0..dim {
        let offset = 4 + i * 8;
        let bytes: [u8; 8] = data[offset..offset + 8].try_into()?;
        embedding.push(f64::from_le_bytes(bytes));
    }

    Ok(embedding)
}

// ─── Hash utilities ─────────────────────────────────────────────────────

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_to_bytes(hex: &str) -> Result<[u8; 32]> {
    if hex.len() != 64 {
        anyhow::bail!("Invalid hex length: {}", hex.len());
    }
    let mut result = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk)?;
        result[i] = u8::from_str_radix(s, 16)?;
    }
    Ok(result)
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fnv::FnvHashEmbedder;
    use tempfile::tempdir;

    #[tokio::test]
    async fn cache_hit_avoids_computation() {
        let dir = tempdir().unwrap();
        let cached = CachedEmbedder::new(FnvHashEmbedder, dir.path().to_path_buf());

        // First call computes
        let emb1 = cached.embed("hello world").await.unwrap();
        let size_after_first = cached.cache_size().await;
        assert_eq!(size_after_first, 1);

        // Second call should hit cache
        let emb2 = cached.embed("hello world").await.unwrap();
        assert_eq!(emb1, emb2);
        assert_eq!(cached.cache_size().await, 1);
    }

    #[tokio::test]
    async fn different_texts_get_different_cache_entries() {
        let dir = tempdir().unwrap();
        let cached = CachedEmbedder::new(FnvHashEmbedder, dir.path().to_path_buf());

        cached.embed("hello").await.unwrap();
        cached.embed("world").await.unwrap();
        assert_eq!(cached.cache_size().await, 2);
    }

    #[tokio::test]
    async fn cache_survives_restart() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().to_path_buf();

        // First instance: compute and cache
        {
            let cached = CachedEmbedder::new(FnvHashEmbedder, cache_path.clone());
            cached.embed("persistent test").await.unwrap();
            assert_eq!(cached.cache_size().await, 1);
        }

        // Second instance: should load from disk
        {
            let cached = CachedEmbedder::new(FnvHashEmbedder, cache_path);
            assert_eq!(cached.cache_size().await, 1, "Cache should survive restart");
        }
    }

    #[test]
    fn write_and_read_embedding_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_cache");
        let original = vec![0.1, 0.2, 0.3, 0.4, 0.5];

        write_embedding(&path, &original).unwrap();
        let loaded = read_embedding(&path).unwrap();

        assert_eq!(loaded, original);
    }

    #[test]
    fn hex_conversion_roundtrip() {
        let original = sha256_bytes(b"test data");
        let hex = bytes_to_hex(&original);
        let recovered = hex_to_bytes(&hex).unwrap();
        assert_eq!(original, recovered);
    }

    #[tokio::test]
    async fn clear_cache_works() {
        let dir = tempdir().unwrap();
        let cached = CachedEmbedder::new(FnvHashEmbedder, dir.path().to_path_buf());

        cached.embed("test").await.unwrap();
        assert_eq!(cached.cache_size().await, 1);

        cached.clear_cache().await.unwrap();
        assert_eq!(cached.cache_size().await, 0);
    }
}
