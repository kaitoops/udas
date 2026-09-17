//! # Data Vault — Big Data Storage Scheduler
//!
//! A layered storage system for DeepSeek TUI (v4pro) that provides
//! structured logging, archival, and analytics for all human-AI
//! and agent-loop interactions.
//!
//! ## Layers
//!
//! | Layer | Name | Storage | Access |
//! |------|------|---------|--------|
//! | L0 | index.db | SQLite | Every query entry |
//! | L1 | hot | JSONL | ~60% of queries |
//! | L2 | warm | gzip | ~25% of queries |
//! | L3 | cold | gzip+summary | ~10% of queries |
//! | L4 | agent-display | JSONL(no-ANSI) | Agent lifecycle |
//! | L5 | session-bridge | SQLite pointers | Session lookup |
//! | L6 | memory-history | JSONL+baseline | Memory changes |
//! | L7 | audit-extension | SQLite+JSONL | Sensitive ops |
//! | L8 | recall-index | BM25 index | Full-text search |
//! | L9 | analytics | aggregated JSON | Statistics |
//!
//! ## Migration
//!
//! Primary: event-driven (e1 session close, e2 agent close, e3 cycle, e4 daily)
//! Secondary: space-driven (s1 hot > 500MB, s2 warm > 2GB, s3 disk > 80%, s4 index > 500MB)

pub mod archive;
pub mod config;
pub mod index;
pub mod layers;
pub mod migration;
pub mod monitor;
pub mod stats;

use anyhow::{Context, Result};
use config::DataVaultConfig;
use index::{IndexDb, IndexEntry};
use migration::{MigrationEngine, MigrationEvent, MigrationReport};
use stats::AnalyticsReport;
use std::sync::Arc;

/// The main entry point for interacting with the Data Vault.
pub struct Vault {
    pub config: DataVaultConfig,
    pub index: Arc<IndexDb>,
    pub migration: MigrationEngine,
}

impl Vault {
    /// Open the Data Vault with the given configuration.
    /// Ensures directories exist and the index database is initialized.
    pub fn open(config: DataVaultConfig) -> Result<Self> {
        let resolved = config.resolved_path();
        std::fs::create_dir_all(&resolved)
            .with_context(|| format!("failed to create vault dir: {}", resolved.display()))?;
        std::fs::create_dir_all(&config.hot_dir())?;
        std::fs::create_dir_all(&config.warm_dir())?;
        std::fs::create_dir_all(&config.cold_dir())?;
        std::fs::create_dir_all(&config.agent_display_dir())?;

        let index_path = config.index_db_path();
        std::fs::create_dir_all(index_path.parent().unwrap())?;
        let index = Arc::new(IndexDb::open(index_path)?);
        let migration = MigrationEngine::new(config.clone(), index.clone());

        Ok(Self { config, index, migration })
    }

    /// Append a log entry to the hot layer (L1) and index it.
    pub fn log_entry(&self, entry: IndexEntry) -> Result<()> {
        // Write to hot JSONL
        let hot_path = self.config.hot_dir().join(format!("{}.jsonl", &entry.ts[..10]));
        let json_value = serde_json::to_value(&entry)?;
        archive::append_jsonl(&hot_path, &json_value)
            .context("failed to write hot jsonl")?;
        // Index it
        self.index.insert(&entry)?;
        Ok(())
    }

    /// Search the vault by text query (L0 index + L8 fallback).
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<IndexEntry>> {
        self.index.search_text(query, limit)
    }

    /// List all sessions that have entries in the vault.
    pub fn list_sessions(&self) -> Result<Vec<String>> {
        self.index.list_sessions()
    }

    /// Get aggregated statistics.
    pub fn stats(&self) -> Result<AnalyticsReport> {
        stats::compute_stats(&self.index)
    }

    /// Fire a migration event.
    pub fn handle_event(&self, event: MigrationEvent) -> MigrationReport {
        self.migration.handle_event(event)
    }

    /// Run daily maintenance (e4 + space checks).
    pub fn daily_maintenance(&self) -> MigrationReport {
        self.migration.handle_event(MigrationEvent::DailyMaintenance)
    }

    /// Get a health summary.
    pub fn health(&self) -> HealthReport {
        let hot_size = dir_size_std(&self.config.hot_dir());
        let warm_size = dir_size_std(&self.config.warm_dir());
        let cold_size = dir_size_std(&self.config.cold_dir());
        let index_count = self.index.count().unwrap_or(0);
        let index_path = self.config.index_db_path();
        let index_file_size = std::fs::metadata(&index_path).map(|m| m.len()).unwrap_or(0);

        HealthReport {
            status: "RUNNING".to_string(),
            hot_mb: hot_size / (1024 * 1024),
            hot_max_mb: self.config.max_hot_mb,
            warm_mb: warm_size / (1024 * 1024),
            warm_max_mb: self.config.max_warm_mb,
            cold_mb: cold_size / (1024 * 1024),
            index_mb: (index_file_size / (1024 * 1024)) as u32,
            index_max_mb: self.config.index_max_mb,
            index_entries: index_count,
            warnings: self.migration.check_thresholds(),
        }
    }
}

/// Human-readable health status.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthReport {
    pub status: String,
    pub hot_mb: u64,
    pub hot_max_mb: u32,
    pub warm_mb: u64,
    pub warm_max_mb: u32,
    pub cold_mb: u64,
    pub index_mb: u32,
    pub index_max_mb: u32,
    pub index_entries: i64,
    pub warnings: Vec<String>,
}

fn dir_size_std(path: &std::path::Path) -> u64 {
    path.read_dir().map(|entries| {
        entries.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
    }).unwrap_or(0)
}

impl std::fmt::Display for HealthReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Data Vault: {}", self.status)?;
        writeln!(f, "  Hot:    {} MB / {} MB", self.hot_mb, self.hot_max_mb)?;
        writeln!(f, "  Warm:   {} MB / {} MB", self.warm_mb, self.warm_max_mb)?;
        writeln!(f, "  Cold:   {} MB / ∞", self.cold_mb)?;
        writeln!(f, "  Index:  {} MB / {} MB  ({} entries)", self.index_mb, self.index_max_mb, self.index_entries)?;
        if !self.warnings.is_empty() {
            writeln!(f, "  Warnings:")?;
            for w in &self.warnings {
                writeln!(f, "    ⚠ {}", w)?;
            }
        }
        Ok(())
    }
}
