//! HARNESS — Hierarchical Agent Runtime NEgotiation & Structured Supervisor
//!
//! Three-agent orchestration system:
//!   Planner (PRO)  → break down user intent into structured contracts
//!   Evaluator (Flash) → review contracts + validate outputs
//!   Generator (Flash)  → execute contracts, deliver artifacts
//!
//! Phase 1 (current): Coordinator + Generator minimum viable chain.
//! Phase 2: Evaluator contract review + output validation.
//! Phase 3: Full three-agent closed loop with gated pipeline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Root of the HARNESS system on disk.
///
/// Defaults to `~/WorkBuddy/<latest>/_dot-workbuddy-core/harness-system/harness/`
/// for per-installation isolation; fall back to a workspace-relative path when
/// the home directory is unavailable.
static HARNESS_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Resolve the harness root directory.  Checks the `HARNESS_ROOT` environment
/// variable first, then walks `~/WorkBuddy/` looking for the latest dated
/// directory containing `_dot-workbuddy-core/`, and finally falls back to the
/// current workspace.
pub fn harness_root() -> &'static PathBuf {
    HARNESS_ROOT.get_or_init(|| {
        if let Ok(env) = std::env::var("HARNESS_ROOT") {
            let p = PathBuf::from(env);
            if p.is_dir() {
                return p;
            }
        }
        // Walk ~/WorkBuddy/ for the latest dated subdirectory
        if let Some(home) = dirs::home_dir() {
            let wb = home.join("WorkBuddy");
            if wb.is_dir() {
                if let Ok(mut entries) = std::fs::read_dir(&wb) {
                    let mut candidates: Vec<PathBuf> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| {
                            p.is_dir()
                                && p.join("_dot-workbuddy-core")
                                    .join("harness-system")
                                    .join("harness")
                                    .is_dir()
                        })
                        .collect();
                    candidates.sort();
                    if let Some(latest) = candidates.last() {
                        return latest.join("_dot-workbuddy-core/harness-system/harness");
                    }
                }
            }
        }
        // Final fallback: workspace-relative
        PathBuf::from("_dot-workbuddy-core/harness-system/harness")
    })
}

// ── Agent definitions ────────────────────────────────────────────────────────

/// Canonical agent identifiers in the HARNESS pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    /// Strategist and task manager — PRO model.
    #[serde(rename = "planner")]
    Planner,
    /// Quality gate — Flash model.
    #[serde(rename = "evaluator")]
    Evaluator,
    /// Executor / deliverable producer — Flash model.
    #[serde(rename = "generator")]
    Generator,
}

impl AgentRole {
    pub fn model(&self) -> &'static str {
        match self {
            Self::Planner => "deepseek-v4-pro",
            Self::Evaluator => "deepseek-v4-flash",
            Self::Generator => "deepseek-v4-flash",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Planner => "规划器 / Coordinator",
            Self::Evaluator => "评估器 / Evaluator",
            Self::Generator => "生成器 / Generator",
        }
    }

    /// Path to the constraints JSON file for this agent relative to the
    /// harness root.
    pub fn constraints_path(&self) -> PathBuf {
        harness_root()
            .join("agents")
            .join(match self {
                Self::Planner => "planner",
                Self::Evaluator => "evaluator",
                Self::Generator => "generator",
            })
            .join(format!(
                "{}_constraints.json",
                match self {
                    Self::Planner => "planner",
                    Self::Evaluator => "evaluator",
                    Self::Generator => "generator",
                }
            ))
    }
}

// ── Agent constraints ────────────────────────────────────────────────────────

/// Loaded agent constraint file (deserialised from `*_constraints.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConstraints {
    pub agent: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: String,
    pub role: AgentRoleDefinition,
    #[serde(default)]
    pub workflow_stages: Vec<WorkflowStage>,
    #[serde(default)]
    pub contract_review: Option<ReviewConfig>,
    #[serde(default)]
    pub output_review: Option<ReviewConfig>,
    #[serde(default)]
    pub output_types: HashMap<String, OutputTypeConfig>,
    #[serde(default)]
    pub contract_execution: Option<ExecutionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRoleDefinition {
    pub summary: String,
    pub responsibilities: Vec<String>,
    pub forbidden: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStage {
    pub stage: String,
    pub description: String,
    #[serde(default)]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    pub checks: Vec<String>,
    pub outcome: ReviewOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewOutcome {
    #[serde(default)]
    pub passed: String,
    #[serde(default)]
    pub revisions_needed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTypeConfig {
    pub description: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub mode: String,
    pub rule: String,
    pub parallel: bool,
    pub max_concurrent: u32,
}

// ── Contract model ───────────────────────────────────────────────────────────

/// Structured task contract — the unit of delegation in HARNESS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub task_id: String,
    pub objective: String,
    #[serde(rename = "type")]
    pub contract_type: ContractType,
    pub tools_allowed: Vec<String>,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
    pub acceptance_criteria: Vec<String>,
}

fn default_max_steps() -> u32 {
    30
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractType {
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "docs")]
    Docs,
    #[serde(rename = "analysis")]
    Analysis,
}

impl ContractType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "code" => Some(Self::Code),
            "docs" => Some(Self::Docs),
            "analysis" => Some(Self::Analysis),
            _ => None,
        }
    }
}

// ── Harness runtime ──────────────────────────────────────────────────────────

/// Runtime state for a HARNESS pipeline execution.
pub struct HarnessRuntime {
    pub root: PathBuf,
    pub planner_constraints: AgentConstraints,
    pub evaluator_constraints: AgentConstraints,
    pub generator_constraints: AgentConstraints,
}

impl HarnessRuntime {
    /// Initialise the HARNESS runtime by loading all constraint files.
    pub fn init() -> anyhow::Result<Self> {
        let root = harness_root().clone();
        let planner_constraints = load_agent_constraints(&AgentRole::Planner.constraints_path())?;
        let evaluator_constraints =
            load_agent_constraints(&AgentRole::Evaluator.constraints_path())?;
        let generator_constraints =
            load_agent_constraints(&AgentRole::Generator.constraints_path())?;

        Ok(Self {
            root,
            planner_constraints,
            evaluator_constraints,
            generator_constraints,
        })
    }

    /// Build a system prompt fragment describing the generator's constraints
    /// for injection into sub-agent system prompts.
    pub fn generator_prompt_fragment(&self) -> String {
        let c = &self.generator_constraints;
        let forbidden = c
            .role
            .forbidden
            .iter()
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "You are a HARNESS Generator agent.\n\n\
             Role: {}\n\n\
             Responsibilities:\n{}\n\n\
             Forbidden:\n{forbidden}\n\n\
             Execution rule: {}\n",
            c.role.summary,
            c.role
                .responsibilities
                .iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n"),
            c.contract_execution
                .as_ref()
                .map(|e| e.rule.as_str())
                .unwrap_or("Execute one contract at a time, stop when done."),
        )
    }

    /// Resolve the tool allowlist for a given contract type from the generator
    /// constraints.
    pub fn tools_for_contract_type(&self, contract_type: ContractType) -> Vec<String> {
        let type_key = match contract_type {
            ContractType::Code => "code",
            ContractType::Docs => "docs",
            ContractType::Analysis => "analysis",
        };
        self.generator_constraints
            .output_types
            .get(type_key)
            .map(|cfg| cfg.tools.clone())
            .unwrap_or_default()
    }
}

/// Build a system prompt constraint fragment for a given HARNESS agent role.
///
/// Returns `None` when the constraints file cannot be loaded (missing on
/// disk, malformed JSON, etc.) — the caller should fall back to the default
/// sub-agent posture without HARNESS constraints.
pub fn harness_system_prompt_fragment(role: AgentRole) -> Option<String> {
    let path = role.constraints_path();
    let raw = std::fs::read_to_string(&path).ok()?;
    let constraints: AgentConstraints = serde_json::from_str(&raw).ok()?;
    let c = &constraints;
    let forbidden = c
        .role
        .forbidden
        .iter()
        .map(|f| format!("- {f}"))
        .collect::<Vec<_>>()
        .join("\n");
    let responsibilities = c
        .role
        .responsibilities
        .iter()
        .map(|r| format!("- {r}"))
        .collect::<Vec<_>>()
        .join("\n");

    let exec_rule = c
        .contract_execution
        .as_ref()
        .map(|e| e.rule.as_str())
        .unwrap_or("Execute one contract at a time; stop when done.");

    Some(format!(
        "## HARNESS {role_label}\n\n\
         You are a HARNESS **{role_label}** agent.\n\n\
         **Role:** {summary}\n\n\
         **Responsibilities:**\n{responsibilities}\n\n\
         **Forbidden:**\n{forbidden}\n\n\
         **Execution Rule:** {exec_rule}\n\n\
         {contract_note}\n",
        role_label = role.display_name(),
        summary = c.role.summary,
        contract_note = if matches!(role, AgentRole::Generator) {
            "You are a Generator: you receive a contract and produce deliverables. \
             Do NOT plan, do NOT assign work, do NOT evaluate your own output. \
             Read the contract, execute it faithfully, return results."
        } else if matches!(role, AgentRole::Evaluator) {
            "You are an Evaluator: you review contracts and validate outputs. \
             Do NOT generate content, do NOT plan, do NOT make decisions — \
             only produce audit conclusions (passed / revisions_needed)."
        } else {
            "You are a Planner/Coordinator: you break down intent into contracts. \
             Do NOT execute tasks or evaluate quality — only plan and coordinate."
        }
    ))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Load and deserialise a single agent-constraint file.
fn load_agent_constraints(path: &Path) -> anyhow::Result<AgentConstraints> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read harness constraints file {}: {e}",
            path.display()
        )
    })?;
    let constraints: AgentConstraints = serde_json::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse harness constraints file {}: {e}",
            path.display()
        )
    })?;
    Ok(constraints)
}
