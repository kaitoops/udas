//! Hot file management with 3-slot ring-buffer backup.
//!
//! Manages the `UDAS-STATE.md` hot file — the pre-loaded state file
//! that the introspection system reads at the start of every introspection.
//!
//! ## Backup Strategy
//!
//! Before each write, the current file is rotated into a 3-slot backup:
//!
//! ```text
//! Before write:
//!   UDAS-STATE.md        ← current
//!   UDAS-STATE.md.bak-1  ← previous
//!   UDAS-STATE.md.bak-2  ← 2 versions ago
//!   UDAS-STATE.md.bak-3  ← 3 versions ago (oldest, will be overwritten)
//!
//! After rotation:
//!   UDAS-STATE.md        ← NEW content
//!   UDAS-STATE.md.bak-1  ← what was "current"
//!   UDAS-STATE.md.bak-2  ← what was "bak-1"
//!   UDAS-STATE.md.bak-3  ← what was "bak-2"
//! ```
//!
//! This ensures we can always roll back up to 3 versions.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Number of backup slots.
const BACKUP_SLOTS: usize = 3;

/// Manages the UDAS-STATE.md hot file with automatic backup rotation.
pub struct HotFileManager {
    /// Path to the hot file (UDAS-STATE.md).
    hot_file_path: PathBuf,
}

impl HotFileManager {
    /// Create a new manager for the given hot file path.
    pub fn new(hot_file_path: impl Into<PathBuf>) -> Self {
        Self {
            hot_file_path: hot_file_path.into(),
        }
    }

    /// Create a manager using the default path (same directory as the hot file).
    pub fn default_in_dir(dir: impl AsRef<Path>) -> Self {
        let path = dir.as_ref().join("UDAS-STATE.md");
        Self::new(path)
    }

    /// Get the path to the hot file.
    pub fn path(&self) -> &Path {
        &self.hot_file_path
    }

    /// Get the path for a backup slot (1-indexed).
    pub fn backup_path(&self, slot: usize) -> PathBuf {
        assert!(
            (1..=BACKUP_SLOTS).contains(&slot),
            "slot must be 1..={BACKUP_SLOTS}"
        );
        let mut name = self
            .hot_file_path
            .file_name()
            .unwrap_or_default()
            .to_os_string();
        name.push(format!(".bak-{}", slot));
        self.hot_file_path.with_file_name(name)
    }

    /// Read the current hot file content.
    pub fn read(&self) -> Result<String> {
        std::fs::read_to_string(&self.hot_file_path)
            .with_context(|| format!("failed to read hot file: {}", self.hot_file_path.display()))
    }

    /// Check if the hot file exists.
    pub fn exists(&self) -> bool {
        self.hot_file_path.exists()
    }

    /// Write new content to the hot file, rotating the backup first.
    ///
    /// Rotation order: bak-2 → bak-3, bak-1 → bak-2, current → bak-1.
    /// Then writes the new content.
    pub fn write(&self, content: &str) -> Result<()> {
        // Rotate backups: shift each slot up by one.
        // Start from the oldest to avoid overwriting before moving.
        for slot in (1..BACKUP_SLOTS).rev() {
            let from = self.backup_path(slot);
            let to = self.backup_path(slot + 1);
            if from.exists() {
                if to.exists() {
                    std::fs::remove_file(&to).with_context(|| {
                        format!("failed to remove old backup: {}", to.display())
                    })?;
                }
                std::fs::rename(&from, &to).with_context(|| {
                    format!(
                        "failed to rotate backup {} → {}",
                        from.display(),
                        to.display()
                    )
                })?;
            }
        }

        // Move current → bak-1
        if self.hot_file_path.exists() {
            let bak1 = self.backup_path(1);
            if bak1.exists() {
                std::fs::remove_file(&bak1)?;
            }
            std::fs::rename(&self.hot_file_path, &bak1)
                .context("failed to rotate current → bak-1")?;
        }

        // Write new content
        std::fs::write(&self.hot_file_path, content).with_context(|| {
            format!("failed to write hot file: {}", self.hot_file_path.display())
        })?;

        tracing::info!(
            "hot file updated: {} ({} bytes)",
            self.hot_file_path.display(),
            content.len()
        );

        Ok(())
    }

    /// Update a specific section of the hot file.
    ///
    /// Sections are delimited by Markdown headers (`## N. Title`).
    /// If the section exists, its content is replaced. If not, it's appended.
    pub fn update_section(&self, section_header: &str, new_content: &str) -> Result<()> {
        let current = if self.exists() {
            self.read()?
        } else {
            String::new()
        };

        let updated = replace_or_append_section(&current, section_header, new_content);
        self.write(&updated)
    }

    /// Count how many backup slots are currently filled.
    pub fn backup_count(&self) -> usize {
        (1..=BACKUP_SLOTS)
            .filter(|slot| self.backup_path(*slot).exists())
            .count()
    }

    /// Roll back to a specific backup slot (1-indexed).
    ///
    /// This restores the backup content to the hot file.
    /// The backup file itself is not deleted.
    pub fn rollback(&self, slot: usize) -> Result<()> {
        assert!(
            (1..=BACKUP_SLOTS).contains(&slot),
            "slot must be 1..={BACKUP_SLOTS}"
        );
        let backup = self.backup_path(slot);
        if !backup.exists() {
            anyhow::bail!("backup slot {} does not exist: {}", slot, backup.display());
        }
        let content = std::fs::read_to_string(&backup)
            .with_context(|| format!("failed to read backup: {}", backup.display()))?;
        self.write(&content)?;
        tracing::info!("rolled back to backup slot {}", slot);
        Ok(())
    }

    /// List available backups with their metadata.
    pub fn list_backups(&self) -> Vec<BackupInfo> {
        (1..=BACKUP_SLOTS)
            .filter_map(|slot| {
                let path = self.backup_path(slot);
                if path.exists() {
                    let metadata = std::fs::metadata(&path).ok()?;
                    let size = metadata.len();
                    let modified = metadata.modified().ok()?;
                    let modified: chrono::DateTime<chrono::Utc> = modified.into();
                    Some(BackupInfo {
                        slot,
                        path,
                        size_bytes: size,
                        modified_at: modified,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Information about a backup file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupInfo {
    /// Slot number (1-indexed).
    pub slot: usize,
    /// Path to the backup file.
    #[serde(skip)]
    pub path: PathBuf,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Last modified time.
    pub modified_at: chrono::DateTime<chrono::Utc>,
}

/// Replace the content under a section header, or append if not found.
///
/// A section starts with `## header` and ends at the next `## ` or end of file.
fn replace_or_append_section(content: &str, header: &str, new_body: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let header_pattern = format!("## {}", header);

    // Find the section start
    if let Some(start_idx) = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&header_pattern))
    {
        // Find the section end (next ## header or end)
        let end_idx = lines[start_idx + 1..]
            .iter()
            .position(|l| l.trim_start().starts_with("## "))
            .map(|p| start_idx + 1 + p)
            .unwrap_or(lines.len());

        // Rebuild: lines before + header + new body + lines after
        let mut result = String::new();
        // Before section
        for line in &lines[..=start_idx] {
            result.push_str(line);
            result.push('\n');
        }
        // New body
        result.push_str(new_body);
        result.push('\n');
        // After section
        for line in &lines[end_idx..] {
            result.push_str(line);
            result.push('\n');
        }
        result
    } else {
        // Section not found — append
        let mut result = content.to_string();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&format!("## {}\n", header));
        result.push_str(new_body);
        result.push('\n');
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_path() {
        let mgr = HotFileManager::new("/tmp/UDAS-STATE.md");
        assert_eq!(
            mgr.backup_path(1),
            PathBuf::from("/tmp/UDAS-STATE.md.bak-1")
        );
        assert_eq!(
            mgr.backup_path(3),
            PathBuf::from("/tmp/UDAS-STATE.md.bak-3")
        );
    }

    #[test]
    fn test_replace_section_existing() {
        let content = "\
# Title

## 1. First
old content
more old

## 2. Second
second content
";
        let result = replace_or_append_section(content, "1. First", "new content");
        assert!(result.contains("## 1. First"));
        assert!(result.contains("new content"));
        assert!(!result.contains("old content"));
        assert!(result.contains("## 2. Second"));
    }

    #[test]
    fn test_replace_section_not_found() {
        let content = "# Title\n\n## 1. First\ncontent\n";
        let result = replace_or_append_section(content, "9. New", "new section body");
        assert!(result.contains("## 9. New"));
        assert!(result.contains("new section body"));
        assert!(result.contains("## 1. First"));
    }

    #[test]
    fn test_replace_section_last() {
        let content = "# Title\n\n## 1. First\ncontent\n\n## 2. Last\nold last\n";
        let result = replace_or_append_section(content, "2. Last", "new last");
        assert!(result.contains("new last"));
        assert!(!result.contains("old last"));
    }

    #[test]
    fn test_write_and_rotate() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("udas-test-hotfile-{}", std::process::id()));
        let mgr = HotFileManager::new(&test_file);

        // Clean up any existing test files
        for slot in 1..=BACKUP_SLOTS {
            let _ = std::fs::remove_file(mgr.backup_path(slot));
        }
        let _ = std::fs::remove_file(&test_file);

        // Write version 1
        mgr.write("version 1").unwrap();
        assert_eq!(mgr.read().unwrap(), "version 1");
        assert_eq!(mgr.backup_count(), 0);

        // Write version 2
        mgr.write("version 2").unwrap();
        assert_eq!(mgr.read().unwrap(), "version 2");
        assert_eq!(mgr.backup_count(), 1);
        assert_eq!(
            std::fs::read_to_string(mgr.backup_path(1)).unwrap(),
            "version 1"
        );

        // Write version 3
        mgr.write("version 3").unwrap();
        assert_eq!(mgr.read().unwrap(), "version 3");
        assert_eq!(mgr.backup_count(), 2);

        // Write version 4
        mgr.write("version 4").unwrap();
        assert_eq!(mgr.read().unwrap(), "version 4");
        assert_eq!(mgr.backup_count(), 3);

        // Write version 5 — should overwrite bak-3 (oldest)
        mgr.write("version 5").unwrap();
        assert_eq!(mgr.read().unwrap(), "version 5");
        assert_eq!(mgr.backup_count(), 3);
        // bak-1 should be version 4
        assert_eq!(
            std::fs::read_to_string(mgr.backup_path(1)).unwrap(),
            "version 4"
        );
        // bak-2 should be version 3
        assert_eq!(
            std::fs::read_to_string(mgr.backup_path(2)).unwrap(),
            "version 3"
        );
        // bak-3 should be version 2 (version 1 was pushed out)
        assert_eq!(
            std::fs::read_to_string(mgr.backup_path(3)).unwrap(),
            "version 2"
        );

        // Test rollback
        mgr.rollback(1).unwrap();
        assert_eq!(mgr.read().unwrap(), "version 4");

        // Clean up
        let _ = std::fs::remove_file(&test_file);
        for slot in 1..=BACKUP_SLOTS {
            let _ = std::fs::remove_file(mgr.backup_path(slot));
        }
    }

    #[test]
    fn test_list_backups() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("udas-test-hotfile-list-{}", std::process::id()));
        let mgr = HotFileManager::new(&test_file);

        // Clean up
        for slot in 1..=BACKUP_SLOTS {
            let _ = std::fs::remove_file(mgr.backup_path(slot));
        }
        let _ = std::fs::remove_file(&test_file);

        mgr.write("v1").unwrap();
        mgr.write("v2").unwrap();
        mgr.write("v3").unwrap();

        let backups = mgr.list_backups();
        assert_eq!(backups.len(), 2); // bak-1 and bak-2
        assert_eq!(backups[0].slot, 1);
        assert_eq!(backups[1].slot, 2);

        // Clean up
        let _ = std::fs::remove_file(&test_file);
        for slot in 1..=BACKUP_SLOTS {
            let _ = std::fs::remove_file(mgr.backup_path(slot));
        }
    }
}
