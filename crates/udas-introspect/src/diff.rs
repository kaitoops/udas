//! Git diff parsing and change feature extraction.
//!
//! Analyzes `git diff` output to extract structural change features
//! that feed into CSL classification. Does NOT do semantic analysis —
//! only extracts facts about what changed at the code structure level.

use anyhow::Result;
use std::path::PathBuf;

/// Structural features extracted from a git diff.
///
/// These are the inputs to CSL classification. Each field captures
/// a specific type of structural change that maps to a CSL criterion.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ChangeFeatures {
    /// pub fn signatures added (new public functions)
    pub pub_fns_added: Vec<String>,
    /// pub fn signatures removed (deleted public functions)
    pub pub_fns_removed: Vec<String>,
    /// pub fn signatures modified (existing functions with changed signature)
    pub pub_fns_modified: Vec<String>,
    /// Trait implementations added
    pub traits_impl_added: Vec<String>,
    /// Trait implementations removed
    pub traits_impl_removed: Vec<String>,
    /// Dependencies added in Cargo.toml
    pub deps_added: Vec<String>,
    /// Dependencies removed in Cargo.toml
    pub deps_removed: Vec<String>,
    /// Crates added to workspace
    pub crates_added: Vec<String>,
    /// Crates removed from workspace
    pub crates_removed: Vec<String>,
    /// Modules added (pub mod declarations)
    pub modules_added: Vec<String>,
    /// Modules removed
    pub modules_removed: Vec<String>,
    /// Files changed (total count)
    pub files_changed: usize,
    /// Lines added (approximate)
    pub lines_added: usize,
    /// Lines removed (approximate)
    pub lines_removed: usize,
    /// Files that were Cargo.toml (dependency changes)
    pub cargo_toml_changed: bool,
    /// Files that were workspace Cargo.toml
    pub workspace_toml_changed: bool,
    /// Changed file paths (relative)
    pub changed_files: Vec<String>,
    /// Whether all changes are comments/whitespace only (CSL-0 indicator)
    pub comment_only: bool,
}

impl ChangeFeatures {
    /// Check if this diff has any interface-level changes (CSL-2 trigger).
    pub fn has_interface_changes(&self) -> bool {
        !self.pub_fns_added.is_empty()
            || !self.pub_fns_removed.is_empty()
            || !self.traits_impl_added.is_empty()
            || !self.traits_impl_removed.is_empty()
    }

    /// Check if this diff has architectural-level changes (CSL-3 trigger).
    pub fn has_architectural_changes(&self) -> bool {
        !self.crates_added.is_empty()
            || !self.crates_removed.is_empty()
            || !self.modules_added.is_empty()
            || !self.modules_removed.is_empty()
    }

    /// Check if this diff has dependency changes.
    pub fn has_dependency_changes(&self) -> bool {
        !self.deps_added.is_empty() || !self.deps_removed.is_empty()
    }

    /// Summary string for logging.
    pub fn summary(&self) -> String {
        format!(
            "files={}, +{}/-{} lines, pub_fn(+{}/-{}/~{}), traits(+{}/-{}), deps(+{}/-{}), crates(+{}/-{}), modules(+{}/-{})",
            self.files_changed,
            self.lines_added,
            self.lines_removed,
            self.pub_fns_added.len(),
            self.pub_fns_removed.len(),
            self.pub_fns_modified.len(),
            self.traits_impl_added.len(),
            self.traits_impl_removed.len(),
            self.deps_added.len(),
            self.deps_removed.len(),
            self.crates_added.len(),
            self.crates_removed.len(),
            self.modules_added.len(),
            self.modules_removed.len(),
        )
    }

    /// Check if all change features are empty (no changes detected).
    ///
    /// Used by `skip_serializing_if` in CSL result serialization.
    pub fn is_empty(&self) -> bool {
        self.pub_fns_added.is_empty()
            && self.pub_fns_removed.is_empty()
            && self.pub_fns_modified.is_empty()
            && self.traits_impl_added.is_empty()
            && self.traits_impl_removed.is_empty()
            && self.deps_added.is_empty()
            && self.deps_removed.is_empty()
            && self.crates_added.is_empty()
            && self.crates_removed.is_empty()
            && self.modules_added.is_empty()
            && self.modules_removed.is_empty()
            && self.files_changed == 0
            && self.lines_added == 0
            && self.lines_removed == 0
            && !self.comment_only
    }

    /// Quick heuristic CSL level hint based on structural features.
    ///
    /// This is a fast approximation. For full classification with reasons,
    /// use [`CslClassifier::classify`](crate::csl::CslClassifier::classify).
    pub fn csl_level_hint(&self) -> crate::csl::CslLevel {
        if self.has_architectural_changes() || self.workspace_toml_changed {
            crate::csl::CslLevel::Architectural
        } else if self.has_interface_changes() || self.has_dependency_changes() {
            crate::csl::CslLevel::Functional
        } else if !self.comment_only && (self.lines_added > 0 || self.lines_removed > 0) {
            crate::csl::CslLevel::LocalFix
        } else {
            crate::csl::CslLevel::Detail
        }
    }
}

/// Analyzer that parses git diff output into [`ChangeFeatures`].
pub struct DiffAnalyzer {
    /// Working directory for git commands.
    repo_root: PathBuf,
}

impl DiffAnalyzer {
    /// Create a new analyzer for the given repository root.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Run `git diff` and analyze the output.
    ///
    /// If `staged` is true, analyzes staged changes (`git diff --cached`).
    /// Otherwise analyzes unstaged changes (`git diff`).
    pub fn analyze(&self, staged: bool) -> Result<ChangeFeatures> {
        let diff_text = self.get_git_diff(staged)?;
        self.parse_diff(&diff_text)
    }

    /// Analyze a specific commit range (e.g., "HEAD~1..HEAD").
    pub fn analyze_range(&self, range: &str) -> Result<ChangeFeatures> {
        let diff_text = self.run_git(&["diff", range])?;
        self.parse_diff(&diff_text)
    }

    /// Get changed files list (names only, no diff content).
    pub fn changed_files(&self, staged: bool) -> Result<Vec<String>> {
        let args: Vec<&str> = if staged {
            vec!["diff", "--cached", "--name-only"]
        } else {
            vec!["diff", "--name-only"]
        };
        let output = self.run_git(&args)?;
        Ok(output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }

    fn get_git_diff(&self, staged: bool) -> Result<String> {
        let args: Vec<&str> = if staged {
            vec!["diff", "--cached"]
        } else {
            vec!["diff"]
        };
        self.run_git(&args)
    }

    fn run_git(&self, args: &[&str]) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&self.repo_root)
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Parse a git diff text into ChangeFeatures.
    ///
    /// This is a heuristic parser — it looks for patterns in the diff
    /// to identify structural changes. It does NOT parse Rust syntax
    /// perfectly; it uses regex-like pattern matching on added/removed lines.
    pub fn parse_diff(&self, diff_text: &str) -> Result<ChangeFeatures> {
        let mut features = ChangeFeatures::default();
        let mut found_non_comment = false;
        let mut current_is_cargo_toml = false;

        for line in diff_text.lines() {
            // Track changed files
            if line.starts_with("diff --git") {
                features.files_changed += 1;
                // Extract file path: "diff --git a/path b/path"
                current_is_cargo_toml = false;
                if let Some(path) = line.split_whitespace().nth(3) {
                    let path = path.strip_prefix("b/").unwrap_or(path);
                    features.changed_files.push(path.to_string());
                    if path.ends_with("Cargo.toml") {
                        features.cargo_toml_changed = true;
                        current_is_cargo_toml = true;
                        // Check if it's the workspace root Cargo.toml
                        if !path.contains('/') {
                            features.workspace_toml_changed = true;
                        }
                    }
                }
            }

            // Count added/removed lines (exclude diff metadata)
            // Also track whether any non-comment lines were changed
            if line.starts_with('+') && !line.starts_with("+++") {
                features.lines_added += 1;
                let content = &line[1..];
                if !is_comment_or_blank(content) {
                    found_non_comment = true;
                }
            }
            if line.starts_with('-') && !line.starts_with("---") {
                features.lines_removed += 1;
                let content = &line[1..];
                if !is_comment_or_blank(content) {
                    found_non_comment = true;
                }
            }

            // Detect pub fn additions
            if line.starts_with('+') && !line.starts_with("+++") {
                let content = &line[1..];
                if let Some(sig) = extract_pub_fn(content) {
                    features.pub_fns_added.push(sig);
                }
                if let Some(impl_name) = extract_trait_impl(content) {
                    features.traits_impl_added.push(impl_name);
                }
                // Only extract deps and workspace members from Cargo.toml files
                if current_is_cargo_toml {
                    if let Some(dep) = extract_cargo_dep(content) {
                        features.deps_added.push(dep);
                    }
                    if let Some(crate_name) = extract_workspace_member(content) {
                        features.crates_added.push(crate_name);
                    }
                }
                if let Some(mod_name) = extract_pub_mod(content) {
                    features.modules_added.push(mod_name);
                }
            }

            // Detect pub fn removals
            if line.starts_with('-') && !line.starts_with("---") {
                let content = &line[1..];
                if let Some(sig) = extract_pub_fn(content) {
                    features.pub_fns_removed.push(sig);
                }
                if let Some(impl_name) = extract_trait_impl(content) {
                    features.traits_impl_removed.push(impl_name);
                }
                // Only extract deps and workspace members from Cargo.toml files
                if current_is_cargo_toml {
                    if let Some(dep) = extract_cargo_dep(content) {
                        features.deps_removed.push(dep);
                    }
                    if let Some(crate_name) = extract_workspace_member(content) {
                        features.crates_removed.push(crate_name);
                    }
                }
                if let Some(mod_name) = extract_pub_mod(content) {
                    features.modules_removed.push(mod_name);
                }
            }
        }

        // Set comment_only: true if there were changes but all were comments/blank
        features.comment_only =
            !found_non_comment && (features.lines_added > 0 || features.lines_removed > 0);

        // Modified pub fns = intersection of added and removed (signature changed)
        // A function that was both removed and added in the same diff = modified
        let added_set: std::collections::HashSet<_> =
            features.pub_fns_added.iter().cloned().collect();
        let removed_set: std::collections::HashSet<_> =
            features.pub_fns_removed.iter().cloned().collect();
        let modified = added_set
            .intersection(&removed_set)
            .cloned()
            .collect::<Vec<_>>();
        features.pub_fns_modified = modified;

        // Remove modified from added/removed (they were counted in both)
        features
            .pub_fns_added
            .retain(|f| !features.pub_fns_modified.contains(f));
        features
            .pub_fns_removed
            .retain(|f| !features.pub_fns_modified.contains(f));

        Ok(features)
    }
}

/// Extract a pub fn signature from a line of Rust code.
fn extract_pub_fn(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
        // Extract function name (first identifier after "fn ")
        let after_fn = trimmed.find("fn ").map(|i| &trimmed[i + 3..]).unwrap_or("");
        let name = after_fn
            .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if !name.is_empty() {
            return Some(format!("pub fn {}", name));
        }
    }
    None
}

/// Extract a trait implementation from a line of Rust code.
fn extract_trait_impl(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match "impl TraitName for Type" or "impl TraitName for Type<T>"
    if trimmed.starts_with("impl ") && trimmed.contains(" for ") {
        let after_impl = &trimmed[5..];
        let trait_name = after_impl.split(" for ").next().unwrap_or("").trim();
        if !trait_name.is_empty() {
            return Some(format!("impl {}", trait_name));
        }
    }
    None
}

/// Extract a dependency from a Cargo.toml line.
fn extract_cargo_dep(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match "dependency-name = ..." (in [dependencies] section)
    if let Some(eq_pos) = trimmed.find('=') {
        let key = trimmed[..eq_pos].trim();
        // Filter out known non-dependency keys
        if !key.is_empty()
            && !key.starts_with('[')
            && ![
                "version",
                "edition",
                "name",
                "description",
                "license",
                "repository",
                "default-run",
                "rust-version",
                "features",
                "default",
                "path",
            ]
            .contains(&key)
            && !key.starts_with('"')
        {
            return Some(key.to_string());
        }
    }
    None
}

/// Extract a workspace member from workspace Cargo.toml.
fn extract_workspace_member(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match "crates/xxx" in the members array
    if trimmed.starts_with("\"crates/") {
        // Strip trailing comma first, then surrounding quotes
        let cleaned = trimmed.trim_end_matches(',').trim_matches('"');
        let name = cleaned.replace("crates/", "");
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Extract a pub mod declaration.
fn extract_pub_mod(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("pub mod ") {
        let name = trimmed
            .strip_prefix("pub mod ")
            .unwrap_or("")
            .trim_end_matches(';')
            .trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Check if a line of code is a comment or blank (no logic).
///
/// Used to detect CSL-0 (detail-only) changes where the diff
/// only touches comments, whitespace, or formatting.
fn is_comment_or_blank(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Rust line comments: //, //!, ///
    if trimmed.starts_with("//") {
        return true;
    }
    // Rust block comments: /* ... */ or lines starting with *
    if trimmed.starts_with("/*") || trimmed.starts_with("*") {
        return true;
    }
    // TOML comments
    if trimmed.starts_with('#') {
        return true;
    }
    // Markdown comments or frontmatter
    if trimmed.starts_with("<!--") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csl::CslLevel;

    #[test]
    fn test_extract_pub_fn() {
        assert_eq!(
            extract_pub_fn("    pub fn new(config: Config) -> Self {"),
            Some("pub fn new".into())
        );
        assert_eq!(
            extract_pub_fn("    pub async fn embed(&self, text: &str) -> Result<Embedding> {"),
            Some("pub fn embed".into())
        );
        assert_eq!(extract_pub_fn("    fn private() {}"), None);
        assert_eq!(extract_pub_fn("    pub fn"), None);
    }

    #[test]
    fn test_extract_trait_impl() {
        assert_eq!(
            extract_trait_impl("impl Embedder for RemoteEmbedder {"),
            Some("impl Embedder".into())
        );
        assert_eq!(
            extract_trait_impl("impl LlmRestorer for DeepSeekClient {"),
            Some("impl LlmRestorer".into())
        );
        assert_eq!(
            extract_trait_impl("impl Clone for Foo {"),
            Some("impl Clone".into())
        );
        assert_eq!(extract_trait_impl("struct Foo {}"), None);
    }

    #[test]
    fn test_extract_cargo_dep() {
        assert_eq!(
            extract_cargo_dep("udas-embedding = { path = \"../udas-embedding\" }"),
            Some("udas-embedding".into())
        );
        assert_eq!(extract_cargo_dep("serde = \"1.0\""), Some("serde".into()));
        assert_eq!(extract_cargo_dep("version = \"0.8.39\""), None);
        assert_eq!(extract_cargo_dep("[dependencies]"), None);
    }

    #[test]
    fn test_extract_workspace_member() {
        assert_eq!(
            extract_workspace_member("    \"crates/udas-introspect\","),
            Some("udas-introspect".into())
        );
        assert_eq!(
            extract_workspace_member("    \"crates/cli\","),
            Some("cli".into())
        );
        assert_eq!(extract_workspace_member("udas-embedding = { }"), None);
    }

    #[test]
    fn test_extract_pub_mod() {
        assert_eq!(extract_pub_mod("pub mod csl;"), Some("csl".into()));
        assert_eq!(extract_pub_mod("pub mod diff;"), Some("diff".into()));
        assert_eq!(extract_pub_mod("mod private;"), None);
    }

    #[test]
    fn test_parse_diff_detail_only() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1234567..abcdefg 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 //! Module docs
-//! old comment
+//! new comment
";
        let analyzer = DiffAnalyzer::new(".");
        let features = analyzer.parse_diff(diff).unwrap();
        assert_eq!(features.files_changed, 1);
        assert_eq!(features.pub_fns_added.len(), 0);
        assert_eq!(features.lines_added, 1);
        assert_eq!(features.lines_removed, 1);
        assert_eq!(features.csl_level_hint(), CslLevel::Detail);
    }

    #[test]
    fn test_parse_diff_functional_change() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,5 @@
 existing code
+    pub fn new_feature() -> Result<()> {
+        Ok(())
+    }
";
        let analyzer = DiffAnalyzer::new(".");
        let features = analyzer.parse_diff(diff).unwrap();
        assert_eq!(features.pub_fns_added.len(), 1);
        assert!(features.has_interface_changes());
    }

    #[test]
    fn test_change_features_summary() {
        let mut f = ChangeFeatures {
            files_changed: 3,
            lines_added: 50,
            lines_removed: 10,
            ..Default::default()
        };
        f.pub_fns_added.push("pub fn new".into());
        f.deps_added.push("udas-introspect".into());
        let s = f.summary();
        assert!(s.contains("files=3"));
        assert!(s.contains("pub_fn(+1"));
    }

    #[test]
    fn test_has_architectural_changes() {
        let mut f = ChangeFeatures::default();
        f.crates_added.push("new-crate".into());
        assert!(f.has_architectural_changes());

        let mut f2 = ChangeFeatures::default();
        f2.modules_added.push("new_module".into());
        assert!(f2.has_architectural_changes());
    }

    #[test]
    fn test_is_comment_or_blank() {
        assert!(is_comment_or_blank("//! doc comment"));
        assert!(is_comment_or_blank("// regular comment"));
        assert!(is_comment_or_blank("/// doc comment"));
        assert!(is_comment_or_blank("/* block comment */"));
        assert!(is_comment_or_blank(" * continuation"));
        assert!(is_comment_or_blank("# TOML comment"));
        assert!(is_comment_or_blank("   "));
        assert!(is_comment_or_blank(""));
        assert!(!is_comment_or_blank("let x = 5;"));
        assert!(!is_comment_or_blank("pub fn new() {}"));
        assert!(!is_comment_or_blank("println!(\"hello\");"));
    }

    #[test]
    fn test_parse_diff_comment_only() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 //! Module docs
-//! old comment
+//! new comment
";
        let analyzer = DiffAnalyzer::new(".");
        let features = analyzer.parse_diff(diff).unwrap();
        assert!(features.comment_only);
        assert_eq!(features.csl_level_hint(), CslLevel::Detail);
    }

    #[test]
    fn test_parse_diff_code_change_not_comment_only() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -5,3 +5,4 @@
 //! doc
-let old = 1;
+let new = 2;
";
        let analyzer = DiffAnalyzer::new(".");
        let features = analyzer.parse_diff(diff).unwrap();
        assert!(!features.comment_only);
        assert_eq!(features.csl_level_hint(), CslLevel::LocalFix);
    }

    #[test]
    fn test_csl_level_hint_architectural() {
        let mut f = ChangeFeatures::default();
        f.crates_added.push("new-crate".into());
        f.lines_added = 10;
        assert_eq!(f.csl_level_hint(), CslLevel::Architectural);
    }

    #[test]
    fn test_csl_level_hint_functional() {
        let mut f = ChangeFeatures::default();
        f.pub_fns_added.push("pub fn new".into());
        f.lines_added = 5;
        assert_eq!(f.csl_level_hint(), CslLevel::Functional);
    }
}
