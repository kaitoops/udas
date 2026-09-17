/// Configuration for the Data Vault.
///
/// Corresponds to the `[data_vault]` section in `~/.udas/config.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default values for the Data Vault configuration.
pub mod defaults {
    pub fn enabled() -> bool { true }
    pub fn path() -> String { String::from("~/.udas/data-vault/") }
    pub fn hot_retention_hours() -> u32 { 24 }
    pub fn max_hot_mb() -> u32 { 500 }
    pub fn max_warm_mb() -> u32 { 2000 }
    pub fn cold_disk_warn_pct() -> u32 { 80 }
    pub fn index_max_mb() -> u32 { 500 }
}

/// The `[data_vault]` configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DataVaultConfig {
    /// Whether to enable the Data Vault (default: true).
    pub enabled: bool,
    /// Root path for storing vault data.
    pub path: String,
    /// How many hours to keep data in L1 (hot) before event-driven migration.
    pub hot_retention_hours: u32,
    /// Space threshold for L1 (hot) in MB. When exceeded, oldest files are compressed.
    pub max_hot_mb: u32,
    /// Space threshold for L2 (warm) in MB. When exceeded, oldest files are moved to cold.
    pub max_warm_mb: u32,
    /// Disk usage percentage that triggers a warning (L3 cold).
    pub cold_disk_warn_pct: u32,
    /// Index size threshold in MB. When exceeded, detail_text fields are compressed.
    pub index_max_mb: u32,
}

impl Default for DataVaultConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::enabled(),
            path: defaults::path(),
            hot_retention_hours: defaults::hot_retention_hours(),
            max_hot_mb: defaults::max_hot_mb(),
            max_warm_mb: defaults::max_warm_mb(),
            cold_disk_warn_pct: defaults::cold_disk_warn_pct(),
            index_max_mb: defaults::index_max_mb(),
        }
    }
}

impl DataVaultConfig {
    /// Resolve the `~` in the path to the user's home directory.
    pub fn resolved_path(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".udas")
            .join("data-vault")
    }

    /// Get the path to the index database.
    pub fn index_db_path(&self) -> PathBuf {
        self.resolved_path().join("index.db")
    }

    /// Get the path to the hot (L1) directory.
    pub fn hot_dir(&self) -> PathBuf {
        self.resolved_path().join("hot")
    }

    /// Get the path to the warm (L2) directory.
    pub fn warm_dir(&self) -> PathBuf {
        self.resolved_path().join("warm")
    }

    /// Get the path to the cold (L3) directory.
    pub fn cold_dir(&self) -> PathBuf {
        self.resolved_path().join("cold")
    }

    /// Get the path to the L4 agent-display archive.
    pub fn agent_display_dir(&self) -> PathBuf {
        self.resolved_path().join("agent-display")
    }
}
