//! Timeline logging with append-only entries and correction chains.
//!
//! Maintains an append-only JSONL log of significant events (CSL-1+ changes,
//! introspections, corrections). Each entry can optionally reference a prior
//! entry it corrects, forming a correction chain.
//!
//! ## Format
//!
//! Each line is a JSON object:
//!
//! ```json
//! {"id":"evt-001","timestamp":"2026-07-27T17:00:00Z","kind":"csl_classified",
//!  "csl_level":"CSL-2","description":"Added pub fn new_feature","correction_for":null,
//!  "metadata":{"files_changed":1}}
//! ```
//!
//! ## Correction Chains
//!
//! When an entry corrects a previous entry, it sets `correction_for` to the
//! ID of the entry being corrected. This creates an auditable chain:
//!
//! ```text
//! evt-001: "Fixed bug in interference.rs"
//! evt-002: "Correction: the fix was actually in geometry.rs" → corrects evt-001
//! evt-003: "Further correction: both files were involved" → corrects evt-002
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Kind of timeline event.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEntryKind {
    /// A CSL classification was performed.
    CslClassified,
    /// An introspection was performed (self-check, global error scan).
    Introspection,
    /// A hot file update occurred.
    HotFileUpdated,
    /// A correction to a previous entry.
    Correction,
    /// A manual note by the AGENT or user.
    Note,
    /// A defect was discovered.
    DefectFound,
    /// A defect was resolved.
    DefectResolved,
    /// A verification was performed.
    Verification,
}

/// A single entry in the timeline log.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineEntry {
    /// Unique entry ID (e.g., "evt-0001").
    pub id: String,
    /// Timestamp of the event.
    pub timestamp: DateTime<Utc>,
    /// Kind of event.
    pub kind: TimelineEntryKind,
    /// CSL level associated with this event (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csl_level: Option<String>,
    /// Human-readable description.
    pub description: String,
    /// ID of the entry this entry corrects (if this is a correction).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_for: Option<String>,
    /// Additional structured metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl TimelineEntry {
    /// Create a new entry with the given kind and description.
    pub fn new(kind: TimelineEntryKind, description: impl Into<String>) -> Self {
        Self {
            id: String::new(), // Will be assigned by logger
            timestamp: Utc::now(),
            kind,
            csl_level: None,
            description: description.into(),
            correction_for: None,
            metadata: None,
        }
    }

    /// Set the CSL level.
    pub fn with_csl_level(mut self, level: impl Into<String>) -> Self {
        self.csl_level = Some(level.into());
        self
    }

    /// Set the correction target.
    pub fn correcting(entry_id: impl Into<String>) -> Self {
        let mut entry = Self::new(TimelineEntryKind::Correction, String::new());
        entry.correction_for = Some(entry_id.into());
        entry
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Logger that appends entries to a JSONL timeline file.
pub struct TimelineLogger {
    /// Path to the JSONL timeline file.
    log_path: PathBuf,
    /// Counter for generating entry IDs.
    counter: u64,
}

impl TimelineLogger {
    /// Create a new logger for the given log file path.
    pub fn new(log_path: impl Into<PathBuf>) -> Self {
        Self {
            log_path: log_path.into(),
            counter: 0,
        }
    }

    /// Create a logger using the default path in the given directory.
    pub fn default_in_dir(dir: impl AsRef<Path>) -> Self {
        let path = dir.as_ref().join("UDAS-TIMELINE.jsonl");
        Self::new(path)
    }

    /// Get the log file path.
    pub fn path(&self) -> &Path {
        &self.log_path
    }

    /// Initialize the counter by reading existing entries.
    ///
    /// This should be called once before appending, to ensure
    /// entry IDs are sequential and don't collide.
    pub fn init_counter(&mut self) -> Result<()> {
        if !self.log_path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&self.log_path)
            .with_context(|| format!("failed to read timeline: {}", self.log_path.display()))?;
        let count = content.lines().filter(|l| !l.trim().is_empty()).count();
        self.counter = count as u64;
        tracing::debug!("timeline counter initialized to {}", self.counter);
        Ok(())
    }

    /// Append an entry to the timeline.
    ///
    /// The entry's ID is assigned automatically.
    pub fn append(&mut self, mut entry: TimelineEntry) -> Result<String> {
        self.counter += 1;
        let id = format!("evt-{:04}", self.counter);
        entry.id = id.clone();

        let json = serde_json::to_string(&entry).context("failed to serialize timeline entry")?;

        // Append to file (create if not exists)
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .with_context(|| format!("failed to open timeline: {}", self.log_path.display()))?;

        use std::io::Write;
        writeln!(file, "{json}")
            .with_context(|| format!("failed to write timeline: {}", self.log_path.display()))?;

        tracing::debug!("timeline entry appended: {} ({})", id, entry.description);

        Ok(id)
    }

    /// Read all entries from the timeline.
    pub fn read_all(&self) -> Result<Vec<TimelineEntry>> {
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&self.log_path)
            .with_context(|| format!("failed to read timeline: {}", self.log_path.display()))?;
        let mut entries = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: TimelineEntry = serde_json::from_str(line)
                .with_context(|| format!("failed to parse timeline line {}", line_num + 1))?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Read the last N entries.
    pub fn read_last(&self, n: usize) -> Result<Vec<TimelineEntry>> {
        let mut entries = self.read_all()?;
        let start = entries.len().saturating_sub(n);
        entries.drain(0..start);
        Ok(entries)
    }

    /// Find entries that correct the given entry ID.
    pub fn find_corrections(&self, entry_id: &str) -> Result<Vec<TimelineEntry>> {
        let entries = self.read_all()?;
        Ok(entries
            .into_iter()
            .filter(|e| e.correction_for.as_deref() == Some(entry_id))
            .collect())
    }

    /// Get the full correction chain for an entry.
    ///
    /// Returns the entry and all subsequent corrections in order.
    pub fn correction_chain(&self, entry_id: &str) -> Result<Vec<TimelineEntry>> {
        let entries = self.read_all()?;
        let mut chain = Vec::new();

        // Find the original entry
        if let Some(original) = entries.iter().find(|e| e.id == entry_id) {
            chain.push(original.clone());
        }

        // Find all corrections (following the chain)
        let mut current_id = entry_id.to_string();
        loop {
            let correction = entries
                .iter()
                .find(|e| e.correction_for.as_deref() == Some(&current_id));
            match correction {
                Some(c) => {
                    current_id = c.id.clone();
                    chain.push(c.clone());
                }
                None => break,
            }
        }

        Ok(chain)
    }

    /// Count total entries.
    pub fn count(&self) -> Result<usize> {
        if !self.log_path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(&self.log_path)?;
        Ok(content.lines().filter(|l| !l.trim().is_empty()).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_creation() {
        let entry =
            TimelineEntry::new(TimelineEntryKind::CslClassified, "Added pub fn new_feature");
        assert_eq!(entry.kind, TimelineEntryKind::CslClassified);
        assert_eq!(entry.description, "Added pub fn new_feature");
        assert!(entry.correction_for.is_none());
    }

    #[test]
    fn test_entry_with_csl_level() {
        let entry =
            TimelineEntry::new(TimelineEntryKind::CslClassified, "test").with_csl_level("CSL-2");
        assert_eq!(entry.csl_level.as_deref(), Some("CSL-2"));
    }

    #[test]
    fn test_append_and_read() {
        let temp_dir = std::env::temp_dir();
        let log_path = temp_dir.join(format!("udas-test-timeline-{}.jsonl", std::process::id()));

        // Clean up
        let _ = std::fs::remove_file(&log_path);

        let mut logger = TimelineLogger::new(&log_path);
        logger.init_counter().unwrap();

        let id1 = logger
            .append(TimelineEntry::new(
                TimelineEntryKind::CslClassified,
                "first entry",
            ))
            .unwrap();
        let id2 = logger
            .append(TimelineEntry::new(TimelineEntryKind::Note, "second entry"))
            .unwrap();

        assert_eq!(id1, "evt-0001");
        assert_eq!(id2, "evt-0002");

        let entries = logger.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].description, "first entry");
        assert_eq!(entries[1].description, "second entry");

        // Clean up
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn test_correction_chain() {
        let temp_dir = std::env::temp_dir();
        let log_path = temp_dir.join(format!(
            "udas-test-timeline-chain-{}.jsonl",
            std::process::id()
        ));

        let _ = std::fs::remove_file(&log_path);

        let mut logger = TimelineLogger::new(&log_path);
        logger.init_counter().unwrap();

        let id1 = logger
            .append(TimelineEntry::new(
                TimelineEntryKind::CslClassified,
                "original assessment",
            ))
            .unwrap();

        let id2 = logger
            .append(
                TimelineEntry::correcting(&id1).with_metadata(serde_json::json!({
                    "corrected_description": "actually a functional change"
                })),
            )
            .unwrap();

        let id3 = logger
            .append(
                TimelineEntry::correcting(&id2).with_metadata(serde_json::json!({
                    "corrected_description": "actually architectural"
                })),
            )
            .unwrap();

        let chain = logger.correction_chain(&id1).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].id, id1);
        assert_eq!(chain[1].id, id2);
        assert_eq!(chain[2].id, id3);
        assert_eq!(chain[1].correction_for.as_deref(), Some(id1.as_str()));
        assert_eq!(chain[2].correction_for.as_deref(), Some(id2.as_str()));

        // Clean up
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn test_read_last() {
        let temp_dir = std::env::temp_dir();
        let log_path = temp_dir.join(format!(
            "udas-test-timeline-last-{}.jsonl",
            std::process::id()
        ));

        let _ = std::fs::remove_file(&log_path);

        let mut logger = TimelineLogger::new(&log_path);
        logger.init_counter().unwrap();

        for i in 0..5 {
            logger
                .append(TimelineEntry::new(
                    TimelineEntryKind::Note,
                    format!("entry {}", i),
                ))
                .unwrap();
        }

        let last_3 = logger.read_last(3).unwrap();
        assert_eq!(last_3.len(), 3);
        assert_eq!(last_3[0].description, "entry 2");
        assert_eq!(last_3[2].description, "entry 4");

        // Clean up
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn test_counter_init_from_existing() {
        let temp_dir = std::env::temp_dir();
        let log_path = temp_dir.join(format!(
            "udas-test-timeline-init-{}.jsonl",
            std::process::id()
        ));

        let _ = std::fs::remove_file(&log_path);

        // Write some entries
        let mut logger = TimelineLogger::new(&log_path);
        logger.init_counter().unwrap();
        logger
            .append(TimelineEntry::new(TimelineEntryKind::Note, "v1"))
            .unwrap();
        logger
            .append(TimelineEntry::new(TimelineEntryKind::Note, "v2"))
            .unwrap();
        logger
            .append(TimelineEntry::new(TimelineEntryKind::Note, "v3"))
            .unwrap();

        // Create new logger and verify counter picks up from existing
        let mut logger2 = TimelineLogger::new(&log_path);
        logger2.init_counter().unwrap();
        let id = logger2
            .append(TimelineEntry::new(TimelineEntryKind::Note, "v4"))
            .unwrap();
        assert_eq!(id, "evt-0004");

        // Clean up
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn test_find_corrections() {
        let temp_dir = std::env::temp_dir();
        let log_path = temp_dir.join(format!(
            "udas-test-timeline-findcorr-{}.jsonl",
            std::process::id()
        ));

        let _ = std::fs::remove_file(&log_path);

        let mut logger = TimelineLogger::new(&log_path);
        logger.init_counter().unwrap();

        let id1 = logger
            .append(TimelineEntry::new(TimelineEntryKind::Note, "original"))
            .unwrap();
        let _id2 = logger
            .append(TimelineEntry::new(TimelineEntryKind::Note, "unrelated"))
            .unwrap();
        let id3 = logger.append(TimelineEntry::correcting(&id1)).unwrap();

        let corrections = logger.find_corrections(&id1).unwrap();
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].id, id3);

        // Clean up
        let _ = std::fs::remove_file(&log_path);
    }
}
