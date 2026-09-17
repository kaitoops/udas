//! Mentor Mode: AMS (Agent Mentor System) integration for DeepSeek TUI.
//!
//! Provides automatic Handoff template wrapping, lesson injection,
//! and review scoring criteria injection. Activated via the
//! `mentor_mode` parameter in `agent_open`.
//!
//! # Architecture
//!
//! Level 2 code enhancement for the Mentor/Student protocol:
//! - `MentorConfig`: loaded from `~/.udas/config.toml` `[mentor_mode]` section
//! - `wrap_in_handoff_template()`: wraps assignment prompts in structured format
//! - `student_rules()`: behavior rules injected into child system prompt
//! - `load_lessons()`: reads `~/.udas/lessons/*.md` and injects into prompt
//! - `review_criteria()`: scoring template for `agent_eval`
//!
//! # Integration Points
//!
//! 1. `AgentSpawnTool::execute` → call `wrap_in_handoff_template` + `load_lessons`
//! 2. `build_subagent_system_prompt` → append `student_rules()` when active
//! 3. `AgentEvalTool::execute` → append `review_criteria()` when evaluating mentored agents

use std::fs;
use std::path::PathBuf;

/// Mentor mode configuration, loaded from config.toml [mentor_mode] section.
#[derive(Debug, Clone)]
pub struct MentorConfig {
    /// Master switch. When false, no mentor mode features activate.
    pub enabled: bool,
    /// Auto-wrap assignment prompts in Handoff template structure.
    pub auto_handoff: bool,
    /// Auto-load and inject lesson files from this directory.
    pub lessons_dir: Option<PathBuf>,
    /// Auto-inject review scoring criteria in agent_eval.
    pub auto_review: bool,
}

impl Default for MentorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_handoff: true,
            lessons_dir: None,
            auto_review: true,
        }
    }
}

impl MentorConfig {
    /// Load mentor config from the global config directory.
    /// Reads `~/.udas/config.toml` and extracts `[mentor_mode]` section.
    pub fn load() -> Self {
        let config_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".udas");
        let config_path = config_dir.join("config.toml");

        if !config_path.exists() {
            return Self::default();
        }

        let content = match fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        let table: toml::Value = match content.parse() {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };

        let default_section = toml::Value::Table(Default::default());
        let section = table.get("mentor_mode").unwrap_or(&default_section);
        Self::from_toml(section)
    }

    /// Parse from a TOML table value.
    pub fn from_toml(table: &toml::Value) -> Self {
        let enabled = table.get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let auto_handoff = table.get("auto_handoff")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let auto_review = table.get("auto_review")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let lessons_dir = table.get("lessons_dir")
            .and_then(|v| v.as_str())
            .map(|s| expand_tilde(s))
            .or_else(|| {
                // Default: ~/.udas/lessons/
                dirs::home_dir().map(|h| h.join(".udas").join("lessons"))
            });

        Self { enabled, auto_handoff, lessons_dir, auto_review }
    }
}

/// Expand `~` to home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path.starts_with("~\\") {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(&path[2..])
    } else {
        PathBuf::from(path)
    }
}

// === Handoff Template ===

/// Handoff template structure for agent_open prompts.
/// When `mentor_mode=true`, the original prompt is wrapped in this format.
const HANDOFF_TEMPLATE: &str = r#"## Goal
{objective}

## Scope
- Allowed: <files/systems the sub-agent may operate on>
- Do not touch: <forbidden areas>

## Required Evidence
- Evidence path 1: <verification method>
- Evidence path 2: <verification method>

## Execution Rules
- Complete only this phase unless explicitly authorized to continue.
- Stop before public or destructive actions.
- If >10 tool calls or >3 files needed, propose phase split.

## Verification
<verification commands or acceptance criteria>

## Report Format
Return: what changed / files touched / verification result / remaining risk / next phase suggestion.

## Original Task
{original_prompt}"#;

/// Wrap a prompt in Handoff template structure.
///
/// The `objective` is extracted from the first line or summary of the prompt.
/// If `objective` is None, the first non-empty line of `prompt` is used.
pub fn wrap_in_handoff_template(prompt: &str, objective: Option<&str>) -> String {
    let objective = objective.unwrap_or_else(|| {
        prompt.lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Complete the assigned task")
            .trim()
    });
    HANDOFF_TEMPLATE
        .replace("{objective}", objective)
        .replace("{original_prompt}", prompt)
}

// === Student Rules ===

/// Student behavior rules to append to system prompt when mentor_mode is active.
const STUDENT_RULES: &str = r#"

---

## Mentor Mode: Student Behavior Rules

You are operating as a **Student** under a Mentor/Student protocol.

### Self-Check Protocol (MANDATORY)
Before completing your response, verify ALL of the following:

1. **Scope Compliance**: Have you touched ONLY the files/systems listed in the "Allowed" scope?
2. **Evidence Provided**: Have you provided at least 2 independent evidence paths?
3. **Verification Run**: Have you run the verification commands and included their output?
4. **Risk Noted**: Have you listed remaining risks or uncertainties?
5. **Phase Limit**: If >10 tool calls or >3 files, have you proposed a phase split?

### Output Format
Your final message MUST include:
- **What Changed**: List of files modified and what was done
- **Verification**: Commands run and their results
- **Remaining Risk**: Any known issues or incomplete items
- **Next Phase Suggestion**: What should happen next (if applicable)

### Safety Rules
- DO NOT modify files outside the "Allowed" scope without explicit permission
- DO NOT execute destructive commands (rm -rf, DROP TABLE, etc.)
- DO NOT push to remote repositories
- DO NOT send external communications (emails, API calls to third parties)
- When in doubt, STOP and report the uncertainty

### Lesson Compliance
If a Lesson is injected at the beginning of your task, you MUST follow the checklist items. Lessons override default behavior for the specific pattern they address.
"#;

/// Get the Student behavior rules for system prompt injection.
pub fn student_rules() -> &'static str {
    STUDENT_RULES
}

// === Lesson Loading ===

/// Load lesson files from the lessons directory.
///
/// Reads all `*.md` files in `lessons_dir` and concatenates them
/// with proper XML-like delimiters for the model to parse.
/// Returns concatenated lesson content, or empty string if none found.
pub fn load_lessons(lessons_dir: &PathBuf) -> String {
    if !lessons_dir.exists() {
        return String::new();
    }

    let mut lessons = Vec::new();
    if let Ok(entries) = fs::read_dir(lessons_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let filename = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    lessons.push(format!(
                        "<!-- lesson: {} -->\n{}\n<!-- /lesson -->",
                        filename, content.trim()
                    ));
                }
            }
        }
    }

    if lessons.is_empty() {
        String::new()
    } else {
        format!(
            "\n## Injected Lessons (from Mentor)\n\n{}\n",
            lessons.join("\n\n")
        )
    }
}

// === Review Criteria ===

/// Review scoring criteria to append when evaluating a mentored sub-agent.
const REVIEW_CRITERIA: &str = r#"

---

## Mentor Mode: Review Scoring Criteria

When evaluating this sub-agent's output, score on these dimensions (1-5 each):

### 1. Scope Compliance (weight 25%)
- Did the agent ONLY touch files in the "Allowed" scope?
- Did it avoid all "Do not touch" areas?
- Score 5: Perfect compliance | Score 1: Major scope violations

### 2. Evidence Quality (weight 25%)
- Are there at least 2 independent evidence paths?
- Are the evidence paths verifiable (not just claims)?
- Score 5: Strong, independent evidence | Score 1: No evidence, only claims

### 3. Verification Completeness (weight 20%)
- Were verification commands actually run?
- Do the verification results support the claimed changes?
- Score 5: Full verification with passing results | Score 1: No verification attempted

### 4. Risk Awareness (weight 15%)
- Are remaining risks clearly listed?
- Are risks prioritized (high/medium/low)?
- Score 5: Comprehensive risk analysis | Score 1: No risk awareness

### 5. Phase Adherence (weight 15%)
- Did the agent respect the phase boundaries?
- If >10 calls or >3 files, did it propose a split?
- Score 5: Perfect phase discipline | Score 1: Ignored all limits

### Scoring Template
```
Mentor Review Score:
- Scope Compliance: X/5
- Evidence Quality: X/5
- Verification: X/5
- Risk Awareness: X/5
- Phase Adherence: X/5
- Weighted Total: X.X/5

Verdict: Pass (>=3.5) / Needs Changes (2.5-3.4) / Blocked (<2.5)
Findings:
  - [SEVERITY] <description> -- Fix: <action>
Message to Student: <next steps>
```
"#;

/// Get the review criteria for agent_eval injection.
pub fn review_criteria() -> &'static str {
    REVIEW_CRITERIA
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_in_handoff_template() {
        let prompt = "Fix the login bug in auth.ts";
        let result = wrap_in_handoff_template(prompt, None);
        assert!(result.contains("## Goal"));
        assert!(result.contains("Fix the login bug in auth.ts"));
        assert!(result.contains("## Scope"));
        assert!(result.contains("## Required Evidence"));
        assert!(result.contains("## Execution Rules"));
        assert!(result.contains("## Verification"));
        assert!(result.contains("## Report Format"));
    }

    #[test]
    fn test_wrap_with_explicit_objective() {
        let prompt = "Long detailed task description...";
        let result = wrap_in_handoff_template(prompt, Some("Fix auth bug"));
        assert!(result.contains("Fix auth bug"));
        assert!(result.contains("Long detailed task description..."));
    }

    #[test]
    fn test_load_lessons_empty_dir() {
        let dir = PathBuf::from("/tmp/nonexistent_lessons_dir_test");
        let result = load_lessons(&dir);
        assert!(result.is_empty());
    }

    #[test]
    fn test_expand_tilde() {
        let result = expand_tilde("~/test/path");
        assert!(result.starts_with(dirs::home_dir().unwrap().to_str().unwrap()));
    }
}
