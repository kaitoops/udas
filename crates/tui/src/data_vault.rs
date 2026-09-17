/// Data Vault integration for the TUI.
///
/// Initializes the vault at startup and provides global access.
/// All functions are no-ops when the `data-vault` feature is disabled.

use std::sync::OnceLock;

static GLOBAL_VAULT: OnceLock<deepseek_data_vault::Vault> = OnceLock::new();

/// Initialize the Data Vault. Must be called once at TUI startup.
/// Returns `true` if the vault was initialized, `false` if already initialized.
pub fn init_vault() -> bool {
    let config = deepseek_data_vault::config::DataVaultConfig::default();
    match deepseek_data_vault::Vault::open(config) {
        Ok(vault) => {
            GLOBAL_VAULT.set(vault).is_ok()
        }
        Err(e) => {
            tracing::warn!("Data Vault init failed (non-fatal): {e}");
            false
        }
    }
}

/// Get a reference to the global vault (if initialized).
pub fn vault() -> Option<&'static deepseek_data_vault::Vault> {
    GLOBAL_VAULT.get()
}

/// Record an agent display log line in the Data Vault.
/// This is the L4 integration point: ANSI-stripped lines from display_relay.
pub fn record_agent_line(agent_id: &str, line: &str) {
    if let Some(vault) = vault() {
        let entry = deepseek_data_vault::index::IndexEntry {
            id: format!("{}-{}", agent_id, chrono::Utc::now().timestamp_nanos()),
            ts: chrono::Utc::now().to_rfc3339(),
            r#type: "agent_display".to_string(),
            agent_role: None,
            model: None,
            provider: None,
            tokens_prompt: None,
            tokens_completion: None,
            session_id: Some(agent_id.to_string()),
            layer: 4,
            file_path: Some(format!("agent-display/{}/current.jsonl", agent_id)),
            byte_offset: None,
            status: "hot".to_string(),
            goal: None,
            result_status: None,
            duration_ms: None,
            detail_text: Some(line.to_string()),
        };
        let _ = vault.log_entry(entry);
    }
}
