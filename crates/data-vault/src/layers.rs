/// L1-L9 layer definitions and storage paths.
///
/// Each layer has a name, storage format, retention policy, and access pattern.

use crate::config::DataVaultConfig;
use std::path::PathBuf;

/// Metadata about a single Data Vault layer.
#[derive(Debug, Clone)]
pub struct LayerInfo {
    /// Layer number (0-9).
    pub level: u8,
    /// Human-readable name.
    pub name: &'static str,
    /// Directory name under the vault root.
    pub dir_name: &'static str,
    /// Storage format description.
    pub format: &'static str,
    /// Whether this layer stores full content (vs. index-only).
    pub stores_full_content: bool,
    /// Whether this layer is automatically pruned.
    pub auto_prune: bool,
}

impl LayerInfo {
    pub const fn new(level: u8, name: &'static str, dir: &'static str, fmt: &'static str, full: bool, prune: bool) -> Self {
        Self { level, name, dir_name: dir, format: fmt, stores_full_content: full, auto_prune: prune }
    }

    /// Resolve the absolute path for this layer.
    pub fn path(&self, config: &DataVaultConfig) -> PathBuf {
        config.resolved_path().join(self.dir_name)
    }
}

/// All layers in the Data Vault.
pub static ALL_LAYERS: &[LayerInfo] = &[
    LayerInfo::new(0, "index", "index.db", "SQLite", false, false),
    LayerInfo::new(1, "hot", "hot", "JSONL (raw)", true, true),
    LayerInfo::new(2, "warm", "warm", "gzip JSONL", true, false),
    LayerInfo::new(3, "cold", "cold", "gzip + summary", true, false),
    LayerInfo::new(4, "agent-display", "agent-display", "JSONL (no ANSI)", true, false),
    LayerInfo::new(5, "session-bridge", "session-bridge", "SQLite pointers", false, false),
    LayerInfo::new(6, "memory-history", "memory-history", "JSONL delta + baseline", true, false),
    LayerInfo::new(7, "audit-extension", "audit-extension", "SQLite + JSONL", true, false),
    LayerInfo::new(8, "recall-index", "recall-index", "BM25 inverted index", false, true),
    LayerInfo::new(9, "analytics", "analytics", "aggregated JSON", false, false),
];

/// Get a layer by its number.
pub fn layer_by_level(level: u8) -> Option<&'static LayerInfo> {
    ALL_LAYERS.iter().find(|l| l.level == level)
}

/// Ensure all layer directories exist.
pub fn ensure_dirs(config: &DataVaultConfig) -> anyhow::Result<()> {
    let root = config.resolved_path();
    std::fs::create_dir_all(&root)?;
    for layer in ALL_LAYERS {
        if layer.level > 0 && layer.level != 5 && layer.level != 7 && layer.level != 8 && layer.level != 9 {
            std::fs::create_dir_all(layer.path(config))?;
        }
    }
    Ok(())
}
