//! Sub-agent display relay — file-backed output capture for external terminal
//! visualisation.
//!
//! Each sub-agent gets a directory under `~/.udas/agent-display/<agent_id>/`
//! containing:
//! - `output.log` — ANSI-styled output log (real-time progress, thinking blocks,
//!   text responses, tool calls)
//! - `done` — marker file created on completion/failure
//!
//! The `--agent-display <agent-id>` subcommand reads this directory and renders
//! content in a read-only terminal window.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing;

/// File-backed relay that captures sub-agent output for external display.
///
/// All methods are thread-safe (only uses file I/O). The relay is cheap to
/// construct; the heavy work (terminal spawning) only happens once on
/// [`launch_terminal`].
#[derive(Debug)]
pub struct SubAgentDisplayRelay {
    agent_id: String,
    output_path: PathBuf,
    done_path: PathBuf,
    terminal_launched: bool,
}

impl SubAgentDisplayRelay {
    /// Resolve the display root directory.
    /// Creates `~/.udas/agent-display/` if it doesn't exist.
    ///
    /// In tests, the base directory can be overridden via the
    /// `DEEPSEEK_DISPLAY_RELAY_DIR` environment variable so that tests use
    /// a temp directory instead of the real home directory.
    fn display_dir() -> PathBuf {
        let base = match std::env::var("DEEPSEEK_DISPLAY_RELAY_DIR") {
            Ok(overridden) => PathBuf::from(overridden),
            Err(_) => dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".udas")
                .join("agent-display"),
        };
        let _ = fs::create_dir_all(&base);
        base
    }

    /// Create a new display relay for the given agent.
    ///
    /// Initialises the output directory (creating it if needed) and writes the
    /// header line. This is cheap and non-blocking.
    pub fn new(agent_id: &str) -> Self {
        let dir = Self::display_dir().join(agent_id);
        let _ = fs::create_dir_all(&dir);
        let output_path = dir.join("output.log");
        let done_path = dir.join("done");

        // Write header so the display client sees something immediately.
        if let Ok(mut f) = fs::File::create(&output_path) {
            let _ = writeln!(
                f,
                "\x1b[1m\x1b[36m═══ Sub-Agent: {agent_id} ═══\x1b[0m\n"
            );
        }

        Self {
            agent_id: agent_id.to_string(),
            output_path,
            done_path,
            terminal_launched: false,
        }
    }

    /// Path to the output log (for the display client to read).
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Append a raw line (with ANSI escapes) to the log file.
    fn append(&self, line: &str) {
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_path)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    // ── Public write methods ────────────────────────────────────────────

    /// Log a progress status update (e.g. "step 3/20: requesting model").
    pub fn write_status(&self, status: &str) {
        self.append(&format!(
            "\x1b[34m[STATUS]\x1b[0m {status}"
        ));
    }

    /// Log a thinking (reasoning) block from the LLM.
    /// Rendered in dim yellow for visual distinction.
    pub fn write_thinking(&self, thinking: &str) {
        for line in thinking.lines() {
            self.append(&format!(
                "\x1b[2m\x1b[33m[THINK]\x1b[0m\x1b[2m {}\x1b[0m",
                line
            ));
        }
    }

    /// Log a text response block from the LLM.
    pub fn write_text(&self, text: &str) {
        for line in text.lines() {
            self.append(&format!("[TEXT] {line}"));
        }
    }

    /// Append a text delta (incremental, no newline) to the log file.
    /// Used for streaming output — each chunk is appended without a trailing newline.
    pub fn write_text_delta(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_path)
        {
            let _ = write!(f, "{text}");
        }
    }

    /// Append a thinking delta (incremental, no newline) to the log file.
    /// Used for streaming thinking blocks — each chunk is appended without
    /// a trailing newline so thinking flows as a continuous block.
    pub fn write_thinking_delta(&self, thinking: &str) {
        if thinking.is_empty() {
            return;
        }
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_path)
        {
            let _ = write!(f, "{thinking}");
        }
    }

    /// Log a tool call.
    pub fn write_tool_call(&self, tool_name: &str, step: u32) {
        self.append(&format!(
            "\x1b[35m[TOOL]\x1b[0m step {step}: \x1b[1m{tool_name}\x1b[0m"
        ));
    }

    /// Log a tool result.
    pub fn write_tool_result(&self, tool_name: &str, ok: bool) {
        let icon = if ok { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
        self.append(&format!("{icon} {tool_name}"));
    }

    /// Log successful completion, then write the done marker.
    pub fn write_complete(&self, summary: &str) {
        self.append(&format!(
            "\n\x1b[1m\x1b[32m═══ COMPLETE ═══\x1b[0m\n\x1b[32m{}\x1b[0m",
            summary
        ));
        let _ = fs::write(&self.done_path, "done");
    }

    /// Log failure, then write the done marker.
    pub fn write_failed(&self, error: &str) {
        self.append(&format!(
            "\n\x1b[1m\x1b[31m═══ FAILED ═══\x1b[0m\n\x1b[31m{}\x1b[0m",
            error
        ));
        let _ = fs::write(&self.done_path, "failed");
    }

    // ── Terminal launch ─────────────────────────────────────────────────

    /// Maximum number of concurrent display windows allowed.
    const MAX_ACTIVE_DISPLAYS: usize = 20;

    /// Count recent active directories — only directories that have had
    /// `output.log` modified within the last 30 minutes are counted.
    /// This prevents orphan directories from broken sessions (no "done" marker)
    /// from blocking window launches in the current session.
    pub fn active_display_count() -> usize {
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return 0,
        };
        let base = home.join(".udas").join("agent-display");
        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(1800))
            .unwrap_or(std::time::UNIX_EPOCH);
        if let Ok(entries) = fs::read_dir(&base) {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| {
                    // Count as active only if output.log was modified recently
                    // AND has no "done" marker
                    let log = e.path().join("output.log");
                    if let Ok(meta) = fs::metadata(&log) {
                        if let Ok(mtime) = meta.modified() {
                            if mtime < cutoff {
                                return false; // Too old, skip
                            }
                        }
                    }
                    !e.path().join("done").exists()
                })
                .count()
        } else {
            0
        }
    }

    /// Best-effort launch of a new terminal window showing this agent's
    /// output. Safe to call multiple times — only the first call spawns a
    /// window.
    ///
    /// If the number of active display windows has reached the limit
    /// (`MAX_ACTIVE_DISPLAYS`), no new window is launched — the relay
    /// still writes to the log file so the user can view it later.
    ///
    /// The terminal runs `deepseek-tui --agent-display <agent-id>` which
    /// reads and follows the output log in read-only mode.
    pub fn launch_terminal(&mut self) {
        if self.terminal_launched {
            return;
        }

        // Check active window count before launching
        let active = Self::active_display_count();
        tracing::trace!(
            "active display windows: {}/{}",
            active,
            Self::MAX_ACTIVE_DISPLAYS,
        );
        if active >= Self::MAX_ACTIVE_DISPLAYS {
            tracing::info!(
                "skipping terminal launch for agent {}: {}/{} active displays (limit reached)",
                self.agent_id,
                active,
                Self::MAX_ACTIVE_DISPLAYS,
            );
            return;
        }

        self.terminal_launched = true;

        let agent_id = &self.agent_id;
        let our_exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(_) => return,
        };
        let exe_str = our_exe.to_string_lossy().to_string();

        #[cfg(target_os = "windows")]
        {
            // Windows: start "" "program" args
            // 技巧：第一个引号参数是窗口标题，用空标题 "" 避免路径歧义
            // 不能拼接成单个字符串，必须拆开参数传递给 cmd /C
            let _ = match Command::new("cmd")
                .args([
                    "/C",
                    "start",
                    "",               // 空标题（必须，否则路径含空格会解析错误）
                    &exe_str,          // 程序路径
                    "agent-display",   // 子命令
                    agent_id,          // agent ID
                ])
                .spawn()
            {
                Ok(child) => {
                    tracing::info!("launched display window for agent {agent_id} (pid={})", child.id());
                    child
                }
                Err(e) => {
                    tracing::warn!("failed to launch display window for agent {agent_id}: {e}");
                    return;
                }
            };
        }

        #[cfg(target_os = "macos")]
        {
            // macOS: `open -a Terminal deepseek-tui --args agent-display <id>`
            let _ = Command::new("open")
                .args([
                    "-a",
                    "Terminal",
                    &exe_str,
                    "--args",
                    "agent-display",
                    agent_id,
                ])
                .spawn();
        }

        #[cfg(target_os = "linux")]
        {
            // Linux: try common terminal emulators
            let cmd = format!("{exe_str} agent-display {agent_id}");
            for term in &[
                "x-terminal-emulator",
                "gnome-terminal",
                "xterm",
                "konsole",
                "xfce4-terminal",
                "lxterminal",
                "urxvt",
            ] {
                if Command::new(term).args(["-e", &cmd]).spawn().is_ok() {
                    break;
                }
            }
        }
    }
}

/// Maximum number of display windows that can be open simultaneously.
/// If more sub-agents are spawned, they still write relay files but
/// no new terminal windows are launched.
pub const MAX_DISPLAY_WINDOWS: usize = 20;

/// Returns the number of currently active display windows.
///
/// An "active" window is a directory under `~/.udas/agent-display/`
/// that does NOT have a `done` marker file AND had `output.log` modified
/// within the last 30 minutes (ignoring orphan directories from broken sessions).
pub fn active_window_count() -> usize {
    let dir = SubAgentDisplayRelay::display_dir();
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(1800))
        .unwrap_or(std::time::UNIX_EPOCH);
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Skip orphan directories: no output.log or output.log too old
        let log = path.join("output.log");
        if let Ok(meta) = fs::metadata(&log) {
            if let Ok(mtime) = meta.modified() {
                if mtime < cutoff {
                    continue; // Too old, orphan from broken session
                }
            }
        } else {
            continue; // No output.log at all
        }
        if !path.join("done").exists() {
            count += 1;
        }
    }
    count
}

/// Clean up stale display files for agents that no longer appear in the
/// manager's agent list. Keeps the display directory tidy.
pub fn clean_stale_displays(active_agent_ids: &[String]) {
    let dir = SubAgentDisplayRelay::display_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !active_agent_ids.contains(&name.to_string()) {
                // Only remove directories with a done marker (completed agents)
                if path.join("done").exists() {
                    let _ = fs::remove_dir_all(&path);
                }
            }
        }
    }
}
