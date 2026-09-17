//! CSL (Change Semantic Level) classification logic.
//!
//! Classifies code changes into four semantic levels based on structural
//! features extracted from git diffs. The classification is deterministic
//! and based on objective criteria — no LLM involvement.
//!
//! ## Classification Algorithm
//!
//! Top-down evaluation: check the highest impact level first.
//!
//! 1. **CSL-3 (Architectural)**: crate/workspace structure changed
//!    - crates added/removed from workspace
//!    - modules added/removed
//!    - workspace Cargo.toml changed
//!
//! 2. **CSL-2 (Functional)**: public interface or dependencies changed
//!    - pub fn added/removed/modified
//!    - trait impl added/removed
//!    - dependencies added/removed
//!
//! 3. **CSL-1 (Local Fix)**: code changed but no interface impact
//!    - lines changed but no structural changes
//!    - internal logic, bug fix, parameter tweak
//!
//! 4. **CSL-0 (Detail)**: no code logic changed
//!    - only comments, formatting, typos
//!    - very small diff with no structural markers

use crate::diff::ChangeFeatures;
use chrono::{DateTime, Utc};

/// Semantic level of a code change.
///
/// Higher level = broader impact = more aggressive hot file update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CslLevel {
    /// Detail change (typo, comment, format). No hot file action.
    #[serde(rename = "CSL-0")]
    Detail,

    /// Local fix (bug fix, param tweak). No hot file action, mark in timeline.
    #[serde(rename = "CSL-1")]
    LocalFix,

    /// Functional change (interface/data flow/deps). AGENT evaluates.
    #[serde(rename = "CSL-2")]
    Functional,

    /// Architectural change (crate reorg, direction). Force update.
    #[serde(rename = "CSL-3")]
    Architectural,
}

impl CslLevel {
    /// Numeric value for comparison (0 = lowest impact, 3 = highest).
    pub fn as_num(&self) -> u8 {
        match self {
            CslLevel::Detail => 0,
            CslLevel::LocalFix => 1,
            CslLevel::Functional => 2,
            CslLevel::Architectural => 3,
        }
    }

    /// Whether this level requires hot file update.
    pub fn requires_hot_file_update(&self) -> bool {
        match self {
            CslLevel::Detail | CslLevel::LocalFix => false,
            CslLevel::Functional => false,   // AGENT evaluates
            CslLevel::Architectural => true, // Force update
        }
    }

    /// Whether this level requires timeline logging.
    pub fn requires_timeline_log(&self) -> bool {
        match self {
            CslLevel::Detail => false, // Too trivial to log
            CslLevel::LocalFix | CslLevel::Functional | CslLevel::Architectural => true,
        }
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            CslLevel::Detail => "CSL-0 Detail",
            CslLevel::LocalFix => "CSL-1 Local Fix",
            CslLevel::Functional => "CSL-2 Functional",
            CslLevel::Architectural => "CSL-3 Architectural",
        }
    }
}

impl std::fmt::Display for CslLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Result of CSL classification.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CslResult {
    /// Classified semantic level.
    pub level: CslLevel,
    /// Human-readable reasons explaining why this level was chosen.
    pub reasons: Vec<String>,
    /// The change features that were analyzed.
    #[serde(skip_serializing_if = "ChangeFeatures::is_empty")]
    pub features: ChangeFeatures,
    /// Timestamp of classification.
    pub classified_at: DateTime<Utc>,
    /// Whether hot file update is required.
    pub hot_file_action: HotFileAction,
}

/// What should happen to the hot file after this classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HotFileAction {
    /// Do nothing.
    None,
    /// Mark in timeline only.
    MarkOnly,
    /// AGENT should evaluate whether to update.
    AgentEvaluate,
    /// Force update the hot file.
    ForceUpdate,
}

impl CslResult {
    /// Create a new classification result.
    pub fn new(level: CslLevel, reasons: Vec<String>, features: ChangeFeatures) -> Self {
        let hot_file_action = match level {
            CslLevel::Detail => HotFileAction::None,
            CslLevel::LocalFix => HotFileAction::MarkOnly,
            CslLevel::Functional => HotFileAction::AgentEvaluate,
            CslLevel::Architectural => HotFileAction::ForceUpdate,
        };
        Self {
            level,
            reasons,
            features,
            classified_at: Utc::now(),
            hot_file_action,
        }
    }

    /// Summary string for logging.
    pub fn summary(&self) -> String {
        format!(
            "{} ({:?}) — {}",
            self.level.label(),
            self.hot_file_action,
            self.reasons.join("; ")
        )
    }
}

/// Classifier that determines CSL level from change features.
pub struct CslClassifier;

impl CslClassifier {
    /// Create a new classifier.
    pub fn new() -> Self {
        Self
    }

    /// Classify change features into a CSL level.
    ///
    /// Uses top-down evaluation: check CSL-3 criteria first, then CSL-2,
    /// then CSL-1, then default to CSL-0.
    pub fn classify(&self, features: &ChangeFeatures) -> CslResult {
        // --- CSL-3: Architectural changes ---
        let mut arch_reasons = Vec::new();
        if !features.crates_added.is_empty() {
            arch_reasons.push(format!(
                "crates added: {}",
                features.crates_added.join(", ")
            ));
        }
        if !features.crates_removed.is_empty() {
            arch_reasons.push(format!(
                "crates removed: {}",
                features.crates_removed.join(", ")
            ));
        }
        if !features.modules_added.is_empty() {
            arch_reasons.push(format!(
                "modules added: {}",
                features.modules_added.join(", ")
            ));
        }
        if !features.modules_removed.is_empty() {
            arch_reasons.push(format!(
                "modules removed: {}",
                features.modules_removed.join(", ")
            ));
        }
        if features.workspace_toml_changed {
            arch_reasons.push("workspace Cargo.toml changed".to_string());
        }
        if !arch_reasons.is_empty() {
            return CslResult::new(CslLevel::Architectural, arch_reasons, features.clone());
        }

        // --- CSL-2: Functional changes ---
        let mut func_reasons = Vec::new();
        if !features.pub_fns_added.is_empty() {
            func_reasons.push(format!(
                "pub fn added: {}",
                features.pub_fns_added.join(", ")
            ));
        }
        if !features.pub_fns_removed.is_empty() {
            func_reasons.push(format!(
                "pub fn removed: {}",
                features.pub_fns_removed.join(", ")
            ));
        }
        if !features.pub_fns_modified.is_empty() {
            func_reasons.push(format!(
                "pub fn modified: {}",
                features.pub_fns_modified.join(", ")
            ));
        }
        if !features.traits_impl_added.is_empty() {
            func_reasons.push(format!(
                "trait impl added: {}",
                features.traits_impl_added.join(", ")
            ));
        }
        if !features.traits_impl_removed.is_empty() {
            func_reasons.push(format!(
                "trait impl removed: {}",
                features.traits_impl_removed.join(", ")
            ));
        }
        if !features.deps_added.is_empty() {
            func_reasons.push(format!("deps added: {}", features.deps_added.join(", ")));
        }
        if !features.deps_removed.is_empty() {
            func_reasons.push(format!(
                "deps removed: {}",
                features.deps_removed.join(", ")
            ));
        }
        if !func_reasons.is_empty() {
            return CslResult::new(CslLevel::Functional, func_reasons, features.clone());
        }

        // --- CSL-1: Local fix (code changed but no structural impact) ---
        // If there are actual code changes (lines added/removed beyond
        // comments/formatting), classify as CSL-1.
        let code_lines_added = features.lines_added;
        let code_lines_removed = features.lines_removed;
        if code_lines_added > 0 || code_lines_removed > 0 {
            let mut reasons = vec![format!(
                "code changed (+{}/-{} lines)",
                code_lines_added, code_lines_removed
            )];
            if features.files_changed > 1 {
                reasons.push(format!("{} files affected", features.files_changed));
            }
            return CslResult::new(CslLevel::LocalFix, reasons, features.clone());
        }

        // --- CSL-0: Detail (no code changes, or only whitespace/comments) ---
        CslResult::new(
            CslLevel::Detail,
            vec!["no structural or code logic changes detected".to_string()],
            features.clone(),
        )
    }
}

impl Default for CslClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csl_level_ordering() {
        assert!(CslLevel::Architectural.as_num() > CslLevel::Functional.as_num());
        assert!(CslLevel::Functional.as_num() > CslLevel::LocalFix.as_num());
        assert!(CslLevel::LocalFix.as_num() > CslLevel::Detail.as_num());
    }

    #[test]
    fn test_csl_level_hot_file_action() {
        assert!(!CslLevel::Detail.requires_hot_file_update());
        assert!(!CslLevel::LocalFix.requires_hot_file_update());
        assert!(!CslLevel::Functional.requires_hot_file_update()); // AGENT evaluates
        assert!(CslLevel::Architectural.requires_hot_file_update());
    }

    #[test]
    fn test_csl_level_timeline_log() {
        assert!(!CslLevel::Detail.requires_timeline_log());
        assert!(CslLevel::LocalFix.requires_timeline_log());
        assert!(CslLevel::Functional.requires_timeline_log());
        assert!(CslLevel::Architectural.requires_timeline_log());
    }

    #[test]
    fn test_classify_detail() {
        let features = ChangeFeatures {
            files_changed: 1,
            lines_added: 0,
            lines_removed: 0,
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Detail);
        assert_eq!(result.hot_file_action, HotFileAction::None);
    }

    #[test]
    fn test_classify_local_fix() {
        let features = ChangeFeatures {
            files_changed: 1,
            lines_added: 5,
            lines_removed: 2,
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::LocalFix);
        assert_eq!(result.hot_file_action, HotFileAction::MarkOnly);
        assert!(result.reasons.iter().any(|r| r.contains("code changed")));
    }

    #[test]
    fn test_classify_functional_pub_fn_added() {
        let features = ChangeFeatures {
            files_changed: 1,
            lines_added: 10,
            lines_removed: 0,
            pub_fns_added: vec!["pub fn new_feature".into()],
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Functional);
        assert_eq!(result.hot_file_action, HotFileAction::AgentEvaluate);
    }

    #[test]
    fn test_classify_functional_trait_impl() {
        let features = ChangeFeatures {
            files_changed: 1,
            lines_added: 20,
            traits_impl_added: vec!["impl Embedder".into()],
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Functional);
    }

    #[test]
    fn test_classify_functional_dependency() {
        let features = ChangeFeatures {
            files_changed: 1,
            lines_added: 2,
            deps_added: vec!["udas-introspect".into()],
            cargo_toml_changed: true,
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Functional);
    }

    #[test]
    fn test_classify_architectural_new_crate() {
        let features = ChangeFeatures {
            files_changed: 2,
            lines_added: 15,
            crates_added: vec!["udas-introspect".into()],
            workspace_toml_changed: true,
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Architectural);
        assert_eq!(result.hot_file_action, HotFileAction::ForceUpdate);
    }

    #[test]
    fn test_classify_architectural_new_module() {
        let features = ChangeFeatures {
            files_changed: 1,
            lines_added: 3,
            modules_added: vec!["introspect".into()],
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Architectural);
    }

    #[test]
    fn test_classify_architectural_overrides_functional() {
        // If both architectural and functional changes exist,
        // architectural should win (top-down evaluation).
        let features = ChangeFeatures {
            files_changed: 3,
            lines_added: 50,
            pub_fns_added: vec!["pub fn new_api".into()],
            crates_added: vec!["new-crate".into()],
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Architectural);
    }

    #[test]
    fn test_classify_functional_overrides_local_fix() {
        // If both functional and local-fix changes exist,
        // functional should win.
        let features = ChangeFeatures {
            files_changed: 2,
            lines_added: 30,
            lines_removed: 10,
            pub_fns_modified: vec!["pub fn existing".into()],
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        assert_eq!(result.level, CslLevel::Functional);
    }

    #[test]
    fn test_csl_result_summary() {
        let features = ChangeFeatures {
            files_changed: 2,
            lines_added: 30,
            crates_added: vec!["udas-introspect".into()],
            ..Default::default()
        };
        let classifier = CslClassifier::new();
        let result = classifier.classify(&features);
        let summary = result.summary();
        assert!(summary.contains("CSL-3"));
        assert!(summary.contains("ForceUpdate"));
    }
}
