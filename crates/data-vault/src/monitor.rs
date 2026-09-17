/// API 性能监控 — 记录每次调用并维护运行中统计
///
/// 集成到 Data Vault L9 analytics，提供：
/// - Provider 级延迟分布（P50/P90/P95/P99）
/// - 成功率 / 错误类型分布
/// - Token 吞吐量
/// - 指数退避重试建议
/// - 熔断器状态

use crate::config::DataVaultConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 单次 API 调用记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCallRecord {
    pub timestamp: String,
    pub provider: String,
    pub model: String,
    pub success: bool,
    pub latency_ms: u64,
    pub tokens_prompt: u64,
    pub tokens_completion: u64,
    pub tokens_total: u64,
    pub error_type: Option<String>,
    pub error_detail: Option<String>,
}

/// Provider 级运行中统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStats {
    pub provider: String,
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub success_rate: f64,
    pub avg_latency_ms: f64,
    pub p50_latency_ms: u64,
    pub p90_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub total_tokens: u64,
    pub avg_tokens_per_call: f64,
    pub tokens_per_sec: f64,
    pub circuit_breaker_open: bool,
    pub consecutive_failures: u64,
    pub last_call_ts: Option<String>,
}

/// API 性能监控器
pub struct ApiPerformanceMonitor {
    /// 近期的调用记录（滑动窗口，默认保留最近 100 条）
    recent_calls: Mutex<VecDeque<ApiCallRecord>>,
    /// Provider 级聚合统计
    provider_aggregates: Mutex<BTreeMap<String, ProviderAggregate>>,
    /// 熔断器状态（按 provider）
    circuit_breakers: Mutex<BTreeMap<String, CircuitBreakerState>>,
    /// 配置
    config: DataVaultConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderAggregate {
    latencies: VecDeque<u64>,
    successes: u64,
    failures: u64,
    total_tokens: u64,
    last_count_reset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircuitBreakerState {
    open: bool,
    consecutive_failures: u64,
    last_failure_ts: Option<String>,
    opened_at: Option<String>,
}

impl Default for ProviderAggregate {
    fn default() -> Self {
        Self {
            latencies: VecDeque::with_capacity(500),
            successes: 0,
            failures: 0,
            total_tokens: 0,
            last_count_reset: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self {
            open: false,
            consecutive_failures: 0,
            last_failure_ts: None,
            opened_at: None,
        }
    }
}

impl ApiPerformanceMonitor {
    pub fn new(config: DataVaultConfig) -> Self {
        Self {
            recent_calls: Mutex::new(VecDeque::with_capacity(100)),
            provider_aggregates: Mutex::new(BTreeMap::new()),
            circuit_breakers: Mutex::new(BTreeMap::new()),
            config,
        }
    }

    /// 记录一次 API 调用结果
    pub fn record_call(&self, record: ApiCallRecord) {
        let provider = record.provider.clone();

        // 更新最近调用（滑动窗口）
        {
            let mut recent = self.recent_calls.lock().unwrap();
            if recent.len() >= 100 {
                recent.pop_front();
            }
            recent.push_back(record.clone());
        }

        // 更新 Provider 聚合
        {
            let mut agg_map = self.provider_aggregates.lock().unwrap();
            let agg = agg_map.entry(provider.clone()).or_default();
            if record.success {
                agg.successes += 1;
                agg.latencies.push_back(record.latency_ms);
                if agg.latencies.len() > 500 {
                    agg.latencies.pop_front();
                }
            } else {
                agg.failures += 1;
            }
            agg.total_tokens += record.tokens_total;
        }

        // 更新熔断器
        {
            let mut cb_map = self.circuit_breakers.lock().unwrap();
            let cb = cb_map.entry(provider.clone()).or_default();
            if record.success {
                cb.consecutive_failures = 0;
                // 如果之前是 open 状态且已过恢复时间，半开
                if cb.open {
                    if let Some(opened) = &cb.opened_at {
                        if let Ok(opened_ts) = chrono::DateTime::parse_from_rfc3339(opened) {
                            let elapsed = chrono::Utc::now()
                                .signed_duration_since(opened_ts)
                                .num_seconds();
                            if elapsed > 60 {
                                cb.open = false; // 自动恢复
                            }
                        }
                    }
                }
            } else {
                cb.consecutive_failures += 1;
                cb.last_failure_ts = Some(chrono::Utc::now().to_rfc3339());
                // 连续 3 次失败 → 打开熔断器
                if cb.consecutive_failures >= 3 && !cb.open {
                    cb.open = true;
                    cb.opened_at = Some(chrono::Utc::now().to_rfc3339());
                }
            }
        }
    }

    /// 检查熔断器状态：是否可以发起调用
    pub fn can_call(&self, provider: &str) -> bool {
        let cb_map = self.circuit_breakers.lock().unwrap();
        if let Some(cb) = cb_map.get(provider) {
            if !cb.open {
                return true;
            }
            // open 状态下，检查是否已过 60s 恢复期
            if let Some(opened) = &cb.opened_at {
                if let Ok(opened_ts) = chrono::DateTime::parse_from_rfc3339(opened) {
                    let elapsed =
                        chrono::Utc::now().signed_duration_since(opened_ts).num_seconds();
                    return elapsed > 60; // 60s 后自动半开
                }
            }
            false
        } else {
            true // 从未失败过，当然可以
        }
    }

    /// 获取推荐的重试延迟（指数退避）
    pub fn retry_delay_ms(&self, provider: &str, attempt: u32) -> u64 {
        let cb_map = self.circuit_breakers.lock().unwrap();
        let consecutive = cb_map
            .get(provider)
            .map(|cb| cb.consecutive_failures)
            .unwrap_or(0)
            .max(attempt as u64);
        // 指数退避: 1s, 2s, 4s, 8s, 16s, 32s max
        let delay = 1000u64 * (1u64 << consecutive.min(5));
        delay.min(32_000)
    }

    /// 获取 Provider 的当前统计摘要
    pub fn get_stats(&self, provider: &str) -> Option<ProviderStats> {
        let agg_map = self.provider_aggregates.lock().unwrap();
        let cb_map = self.circuit_breakers.lock().unwrap();

        let agg = agg_map.get(provider)?;
        let cb = cb_map.get(provider);

        let total = agg.successes + agg.failures;
        if total == 0 {
            return None;
        }

        let mut sorted: Vec<u64> = agg.latencies.iter().copied().collect();
        sorted.sort_unstable();
        let len = sorted.len();
        let avg = if len > 0 {
            sorted.iter().copied().sum::<u64>() as f64 / len as f64
        } else {
            0.0
        };

        let total_tokens = agg.total_tokens;
        let total_latency_sec = sorted.iter().copied().sum::<u64>() as f64 / 1000.0;

        Some(ProviderStats {
            provider: provider.to_string(),
            total_calls: total,
            successful_calls: agg.successes,
            failed_calls: agg.failures,
            success_rate: (agg.successes as f64 / total as f64) * 100.0,
            avg_latency_ms: avg,
            p50_latency_ms: sorted.get(len * 50 / 100).copied().unwrap_or(0),
            p90_latency_ms: sorted.get(len * 90 / 100).copied().unwrap_or(0),
            p99_latency_ms: sorted.get(len * 99 / 100).copied().unwrap_or(0),
            total_tokens,
            avg_tokens_per_call: if total > 0 {
                total_tokens as f64 / total as f64
            } else {
                0.0
            },
            tokens_per_sec: if total_latency_sec > 0.0 {
                total_tokens as f64 / total_latency_sec
            } else {
                0.0
            },
            circuit_breaker_open: cb.map(|c| c.open).unwrap_or(false),
            consecutive_failures: cb.map(|c| c.consecutive_failures).unwrap_or(0),
            last_call_ts: None,
        })
    }

    /// 获取所有 Provider 的统计
    pub fn all_stats(&self) -> Vec<ProviderStats> {
        let agg_map = self.provider_aggregates.lock().unwrap();
        let providers: Vec<String> = agg_map.keys().cloned().collect();
        drop(agg_map);
        providers
            .iter()
            .filter_map(|p| self.get_stats(p))
            .collect()
    }
}
