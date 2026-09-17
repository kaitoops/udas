//! Introspect subcommand handler for udas-cli.
//!
//! Provides CLI access to the UDAS introspection system:
//! - CSL classification of git diffs
//! - Hot file (UDAS-STATE.md) read/update/backup management
//! - Timeline logging and correction chain queries

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use std::path::PathBuf;
use udas_introspect::{
    CslClassifier, DiffAnalyzer, HotFileManager, TimelineEntry, TimelineEntryKind, TimelineLogger,
};

/// Introspect subcommands.
#[derive(Subcommand)]
pub enum IntrospectAction {
    /// Classify the current git diff by CSL level.
    ///
    /// Analyzes structural changes and determines whether they are
    /// CSL-0 (detail), CSL-1 (local fix), CSL-2 (functional), or
    /// CSL-3 (architectural).
    Classify {
        /// Analyze staged changes instead of unstaged.
        #[arg(long)]
        staged: bool,
        /// Analyze a specific commit range (e.g., "HEAD~1..HEAD").
        #[arg(long)]
        range: Option<String>,
    },
    /// Read the current hot file (UDAS-STATE.md).
    StateRead,
    /// Update a section of the hot file.
    StateUpdateSection {
        /// Section header (e.g., "6. 内观指针").
        #[arg(long)]
        header: String,
        /// New section content (markdown).
        #[arg(long)]
        content: String,
    },
    /// List available hot file backups.
    StateBackups,
    /// Roll back the hot file to a specific backup slot.
    StateRollback {
        /// Backup slot number (1-3).
        slot: usize,
    },
    /// Read timeline entries.
    TimelineRead {
        /// Read only the last N entries.
        #[arg(long)]
        last: Option<usize>,
    },
    /// Add a timeline entry.
    TimelineAdd {
        /// Entry description.
        #[arg(long)]
        description: String,
        /// Entry kind: csl_classified, introspection, note, defect_found, etc.
        #[arg(long, default_value = "note")]
        kind: String,
        /// CSL level (if applicable): CSL-0, CSL-1, CSL-2, CSL-3.
        #[arg(long)]
        csl_level: Option<String>,
        /// ID of entry this entry corrects.
        #[arg(long)]
        correction_for: Option<String>,
    },
    /// Show the correction chain for a timeline entry.
    TimelineChain {
        /// Entry ID to trace.
        entry_id: String,
    },
}

/// Handle the introspect subcommand.
pub fn handle(action: IntrospectAction) -> Result<()> {
    match action {
        IntrospectAction::Classify { staged, range } => handle_classify(staged, range),
        IntrospectAction::StateRead => handle_state_read(),
        IntrospectAction::StateUpdateSection { header, content } => {
            handle_state_update_section(&header, &content)
        }
        IntrospectAction::StateBackups => handle_state_backups(),
        IntrospectAction::StateRollback { slot } => handle_state_rollback(slot),
        IntrospectAction::TimelineRead { last } => handle_timeline_read(last),
        IntrospectAction::TimelineAdd {
            description,
            kind,
            csl_level,
            correction_for,
        } => handle_timeline_add(
            &description,
            &kind,
            csl_level.as_deref(),
            correction_for.as_deref(),
        ),
        IntrospectAction::TimelineChain { entry_id } => handle_timeline_chain(&entry_id),
    }
}

/// Find the workspace root by looking for UDAS-STATE.md or .git.
fn workspace_root() -> Result<PathBuf> {
    // Try current directory first
    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Walk up to find a directory with UDAS-STATE.md or .git
    let mut current = cwd.as_path();
    loop {
        if current.join("UDAS-STATE.md").exists() || current.join(".git").exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => bail!("could not find workspace root (no UDAS-STATE.md or .git found)"),
        }
    }
}

// --- Handlers ---

fn handle_classify(staged: bool, range: Option<String>) -> Result<()> {
    let root = workspace_root()?;
    let analyzer = DiffAnalyzer::new(&root);

    let features = if let Some(ref r) = range {
        eprintln!("[introspect] analyzing range: {}", r);
        analyzer.analyze_range(r)?
    } else {
        eprintln!(
            "[introspect] analyzing {} changes",
            if staged { "staged" } else { "unstaged" }
        );
        analyzer.analyze(staged)?
    };

    let classifier = CslClassifier::new();
    let result = classifier.classify(&features);

    // Print summary to stderr
    eprintln!("[introspect] features: {}", features.summary());
    eprintln!("[introspect] classification: {}", result.summary());

    // Print JSON result to stdout
    let json = serde_json::to_string_pretty(&result)?;
    println!("{json}");

    Ok(())
}

fn handle_state_read() -> Result<()> {
    let root = workspace_root()?;
    let mgr = HotFileManager::default_in_dir(&root);

    if !mgr.exists() {
        bail!("hot file not found: {}", mgr.path().display());
    }

    let content = mgr.read()?;
    print!("{content}");
    Ok(())
}

fn handle_state_update_section(header: &str, content: &str) -> Result<()> {
    let root = workspace_root()?;
    let mgr = HotFileManager::default_in_dir(&root);

    mgr.update_section(header, content)?;
    eprintln!("[introspect] updated section '{}' in hot file", header);
    eprintln!("[introspect] backups: {} available", mgr.backup_count());
    Ok(())
}

fn handle_state_backups() -> Result<()> {
    let root = workspace_root()?;
    let mgr = HotFileManager::default_in_dir(&root);

    let backups = mgr.list_backups();
    if backups.is_empty() {
        println!("No backups available.");
        return Ok(());
    }

    let output: Vec<serde_json::Value> = backups
        .iter()
        .map(|b| {
            serde_json::json!({
                "slot": b.slot,
                "size_bytes": b.size_bytes,
                "modified_at": b.modified_at.to_rfc3339(),
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn handle_state_rollback(slot: usize) -> Result<()> {
    let root = workspace_root()?;
    let mgr = HotFileManager::default_in_dir(&root);

    if slot < 1 || slot > 3 {
        bail!("slot must be 1, 2, or 3");
    }

    mgr.rollback(slot)?;
    eprintln!("[introspect] rolled back to backup slot {}", slot);
    Ok(())
}

fn handle_timeline_read(last: Option<usize>) -> Result<()> {
    let root = workspace_root()?;
    let logger = TimelineLogger::default_in_dir(&root);

    let entries = if let Some(n) = last {
        logger.read_last(n)?
    } else {
        logger.read_all()?
    };

    if entries.is_empty() {
        println!("No timeline entries found.");
        return Ok(());
    }

    eprintln!(
        "[introspect] {} entries (total: {})",
        entries.len(),
        logger.count()?
    );

    for entry in &entries {
        let json = serde_json::to_string(entry)?;
        println!("{json}");
    }

    Ok(())
}

fn handle_timeline_add(
    description: &str,
    kind_str: &str,
    csl_level: Option<&str>,
    correction_for: Option<&str>,
) -> Result<()> {
    let root = workspace_root()?;
    let mut logger = TimelineLogger::default_in_dir(&root);
    logger.init_counter()?;

    let kind = parse_entry_kind(kind_str)?;

    let mut entry = if let Some(ref_id) = correction_for {
        let mut e = TimelineEntry::correcting(ref_id);
        e.description = description.to_string();
        e
    } else {
        TimelineEntry::new(kind, description)
    };

    if let Some(level) = csl_level {
        entry = entry.with_csl_level(level);
    }

    let id = logger.append(entry)?;
    eprintln!("[introspect] timeline entry added: {}", id);

    // Print the entry as JSON
    let entries = logger.read_last(1)?;
    if let Some(e) = entries.first() {
        println!("{}", serde_json::to_string_pretty(e)?);
    }

    Ok(())
}

fn handle_timeline_chain(entry_id: &str) -> Result<()> {
    let root = workspace_root()?;
    let logger = TimelineLogger::default_in_dir(&root);

    let chain = logger.correction_chain(entry_id)?;

    if chain.is_empty() {
        bail!("entry not found: {}", entry_id);
    }

    eprintln!(
        "[introspect] correction chain for {} ({} entries)",
        entry_id,
        chain.len()
    );

    for entry in &chain {
        let json = serde_json::to_string(entry)?;
        println!("{json}");
    }

    Ok(())
}

/// Parse a timeline entry kind from string.
fn parse_entry_kind(s: &str) -> Result<TimelineEntryKind> {
    match s.to_lowercase().as_str() {
        "csl_classified" | "csl-classified" => Ok(TimelineEntryKind::CslClassified),
        "introspection" => Ok(TimelineEntryKind::Introspection),
        "hot_file_updated" | "hot-file-updated" => Ok(TimelineEntryKind::HotFileUpdated),
        "correction" => Ok(TimelineEntryKind::Correction),
        "note" => Ok(TimelineEntryKind::Note),
        "defect_found" | "defect-found" => Ok(TimelineEntryKind::DefectFound),
        "defect_resolved" | "defect-resolved" => Ok(TimelineEntryKind::DefectResolved),
        "verification" => Ok(TimelineEntryKind::Verification),
        _ => bail!(
            "unknown entry kind: '{}' (valid: csl_classified, introspection, note, defect_found, defect_resolved, verification, correction, hot_file_updated)",
            s
        ),
    }
}
