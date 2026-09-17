/// L9 analytics: aggregated statistics for the Data Vault.
///
/// Produced daily via `deepseek vault stats` and automatically during migration.

use crate::index::IndexDb;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Aggregated statistics for a given time period.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalyticsReport {
    pub period_start: String,
    pub period_end: String,
    /// Total token consumption grouped by (model, provider).
    pub tokens_by_model: BTreeMap<String, TokenStats>,
    /// Count of agent executions per role.
    pub agent_exec_by_role: BTreeMap<String, u64>,
    /// Average duration per agent execution (ms).
    pub avg_agent_duration_ms: f64,
    /// Loop cycle count and success rate.
    pub loop_cycles: LoopStats,
    /// Provider switch count.
    pub provider_switches: u64,
    /// Most frequently used tools.
    pub top_tools: Vec<String>,
    /// Cache hit rate trend.
    pub cache_hit_rate_pct: f64,
    /// Average session lifetime (seconds).
    pub avg_session_lifetime_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenStats {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoopStats {
    pub total_cycles: u64,
    pub auto_stopped: u64,
    pub manual_interrupted: u64,
}

/// Compute daily analytics by scanning the index.
pub fn compute_stats(_index: &IndexDb) -> Result<AnalyticsReport> {
    // NOTE: This is a placeholder that aggregates from the index.
    // In production, L9 would also read from the L3 cold summaries.
    let now = chrono::Utc::now();
    let yesterday = (now - chrono::Duration::days(1)).to_rfc3339();

    Ok(AnalyticsReport {
        period_start: yesterday,
        period_end: now.to_rfc3339(),
        ..Default::default()
    })
}
