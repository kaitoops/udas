/// Event-driven + space-driven migration engine for Data Vault layers.
///
/// Primary: events (e1-e4) trigger migration from hot→warm→cold.
/// Secondary: space thresholds (s1-s4) act as safety net.

use crate::config::DataVaultConfig;
use crate::index::IndexDb;
use std::path::PathBuf;
use std::sync::Arc;

/// Migration events that can be triggered by external components.
pub enum MigrationEvent {
    /// e1: A session has ended.
    SessionClosed { session_id: String },
    /// e2: An agent has finished execution.
    AgentClosed { agent_id: String },
    /// e3: A loop cycle boundary has been reached.
    CycleBoundary { cycle_id: String },
    /// e4: Daily midnight maintenance trigger.
    DailyMaintenance,
}

/// Result of running a migration pass.
#[derive(Debug, Default)]
pub struct MigrationReport {
    pub entries_moved_hot_to_warm: usize,
    pub entries_moved_warm_to_cold: usize,
    pub files_compressed: usize,
    pub index_entries_compressed: usize,
    pub errors: Vec<String>,
}

/// The migration engine.
pub struct MigrationEngine {
    config: DataVaultConfig,
    index: Arc<IndexDb>,
}

impl MigrationEngine {
    pub fn new(config: DataVaultConfig, index: Arc<IndexDb>) -> Self {
        Self { config, index }
    }

    /// Handle a migration event.
    /// This is called by external components when a lifecycle event occurs.
    pub fn handle_event(&self, event: MigrationEvent) -> MigrationReport {
        match event {
            MigrationEvent::SessionClosed { session_id } => {
                self.migrate_session(&session_id, "hot", "warm")
            }
            MigrationEvent::AgentClosed { agent_id } => {
                self.migrate_session(&agent_id, "hot", "warm")
            }
            MigrationEvent::CycleBoundary { cycle_id } => {
                self.migrate_session(&cycle_id, "warm", "cold")
            }
            MigrationEvent::DailyMaintenance => {
                let mut report = MigrationReport::default();
                // e4: compress all remaining hot files
                if let Ok(hot_dir) = self.config.hot_dir().read_dir() {
                    for entry in hot_dir.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |e| e == "jsonl") {
                            if crate::archive::compress_file(&path).is_ok() {
                                report.files_compressed += 1;
                            }
                        }
                    }
                }
                // Check space thresholds (s1-s4)
                let hot_size = dir_size(&self.config.hot_dir());
                if hot_size > (self.config.max_hot_mb as u64) * 1024 * 1024 {
                    // s1: hot too large - trigger warm migration for oldest
                    if let Err(e) = self.compress_oldest_hot() {
                        report.errors.push(format!("s1: {}", e));
                    }
                }
                let warm_size = dir_size(&self.config.warm_dir());
                if warm_size > (self.config.max_warm_mb as u64) * 1024 * 1024 {
                    // s2: warm too large - cold migration
                    if let Err(e) = self.move_oldest_to_cold() {
                        report.errors.push(format!("s2: {}", e));
                    }
                }
                report
            }
        }
    }

    /// Migrate all entries with the given session_id from `from_status` to `to_status`.
    fn migrate_session(&self, session_id: &str, from_status: &str, to_status: &str) -> MigrationReport {
        let mut report = MigrationReport::default();
        match self.index.migrate_status(session_id, from_status, to_status) {
            Ok(n) => {
                if from_status == "hot" && to_status == "warm" {
                    report.entries_moved_hot_to_warm = n;
                } else if from_status == "warm" && to_status == "cold" {
                    report.entries_moved_warm_to_cold = n;
                }
            }
            Err(e) => report.errors.push(format!("migrate {}->{}: {}", from_status, to_status, e)),
        }
        report
    }

    /// Space-driven s1: compress the oldest .jsonl files in hot to .jsonl.gz.
    fn compress_oldest_hot(&self) -> anyhow::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(self.config.hot_dir())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "jsonl"))
            .collect();
        entries.sort_by_key(|e| e.metadata().ok().and_then(|m| m.created().ok()));
        for entry in entries.iter().take(entries.len() / 2) {
            crate::archive::compress_file(&entry.path())?;
        }
        Ok(())
    }

    /// Space-driven s2: move the oldest .jsonl.gz files from warm to cold.
    fn move_oldest_to_cold(&self) -> anyhow::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(self.config.warm_dir())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "gz"))
            .collect();
        entries.sort_by_key(|e| e.metadata().ok().and_then(|m| m.created().ok()));
        let cold_dir = self.config.cold_dir();
        std::fs::create_dir_all(&cold_dir)?;
        for entry in entries.iter().take(entries.len() / 2) {
            let dest = cold_dir.join(entry.file_name());
            std::fs::rename(entry.path(), dest)?;
        }
        Ok(())
    }

    /// Check space thresholds and return warnings.
    pub fn check_thresholds(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let hot = dir_size(&self.config.hot_dir());
        let warm = dir_size(&self.config.warm_dir());
        let hot_max = (self.config.max_hot_mb as u64) * 1024 * 1024;
        let warm_max = (self.config.max_warm_mb as u64) * 1024 * 1024;
        if hot > hot_max {
            warnings.push(format!("HOT exceeds {} MB (actual: {} MB)", self.config.max_hot_mb, hot / 1024 / 1024));
        }
        if warm > warm_max {
            warnings.push(format!("WARM exceeds {} MB", self.config.max_warm_mb));
        }
        warnings
    }
}

fn dir_size(path: &PathBuf) -> u64 {
    path.read_dir().map(|entries| {
        entries.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
    }).unwrap_or(0)
}
