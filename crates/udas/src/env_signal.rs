//! Environmental Signal Conversion Layer
//!
//! Converts environmental signals (time, weather, etc.) into semantic
//! context variations for measurement basis generation.
//!
//! ## Design Principles
//!
//! - **Engine-external**: Part of the memory pool module, not the engine.
//!   The engine receives measurement sets, not raw environmental signals.
//! - **Indirect semantic mapping**: Raw signals (e.g., timestamp) are
//!   converted to semantic context (e.g., "时序语境: 清晨") via indirect
//!   mapping. This avoids the LLM ignoring raw, meaningless values.
//! - **Reliable mappings only**: Only signals with reliable semantic
//!   mappings are used. Signals without clear semantic meaning (e.g.,
//!   CPU load) are discarded.
//! - **True randomness source**: Environmental signals provide genuine
//!   randomness (different times, weather conditions) that creates
//!   measurement basis variations — the "真随机源" for multi-angle
//!   information restoration.

use crate::types::Angle;

// ─── Signal Types ──────────────────────────────────────────────────────

/// Types of environmental signals that can be converted to semantic context.
///
/// Only signals with reliable semantic mappings are included.
/// Signals like CPU load, memory usage, etc. are intentionally excluded
/// because they lack reliable semantic meaning for judgment problems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalType {
    /// Time of day — maps to temporal context (清晨/午后/深夜).
    TimeOfDay,
    /// Day of week — maps to cyclical context (工作日/周末).
    DayOfWeek,
    /// Weather condition — maps to atmospheric context (晴朗/阴沉/雨天).
    Weather,
    /// Season — maps to temporal context (春/夏/秋/冬).
    Season,
    /// User-provided context tag — maps directly to semantic context.
    UserTag,
}

/// A single environmental signal with its raw value.
///
/// The raw value is converted to semantic context via `convert_to_semantic_context()`.
#[derive(Debug, Clone)]
pub struct EnvironmentalSignal {
    pub signal_type: SignalType,
    pub raw_value: String,
}

impl EnvironmentalSignal {
    pub fn new(signal_type: SignalType, raw_value: impl Into<String>) -> Self {
        Self {
            signal_type,
            raw_value: raw_value.into(),
        }
    }

    /// Convert this signal to semantic context (indirect mapping).
    ///
    /// Returns None if the signal has no reliable semantic mapping.
    pub fn to_semantic_context(&self) -> Option<String> {
        convert_to_semantic_context(self)
    }
}

// ─── Semantic Conversion ───────────────────────────────────────────────

/// Convert an environmental signal to semantic context via indirect mapping.
///
/// This is the core of the "间接语义转换" design: raw signals are not
/// injected directly into prompts (which the LLM would ignore), but are
/// mapped to meaningful semantic context that influences the measurement
/// basis.
///
/// Returns None for signals without reliable semantic mappings.
pub fn convert_to_semantic_context(signal: &EnvironmentalSignal) -> Option<String> {
    match signal.signal_type {
        SignalType::TimeOfDay => convert_time_of_day(&signal.raw_value),
        SignalType::DayOfWeek => convert_day_of_week(&signal.raw_value),
        SignalType::Weather => convert_weather(&signal.raw_value),
        SignalType::Season => convert_season(&signal.raw_value),
        SignalType::UserTag => {
            if signal.raw_value.is_empty() {
                None
            } else {
                Some(format!("用户语境: {}", signal.raw_value))
            }
        }
    }
}

/// Convert a time string (HH:MM format) to temporal context.
fn convert_time_of_day(time_str: &str) -> Option<String> {
    // Parse hour from "HH:MM" or "HH" format
    let hour: u32 = time_str
        .split(':')
        .next()
        .and_then(|h| h.trim().parse().ok())
        .unwrap_or(12);

    let context = match hour {
        5..=8 => "清晨时序",
        9..=11 => "上午时序",
        12..=14 => "午后时序",
        15..=17 => "日暮时序",
        18..=21 => "黄昏时序",
        _ => "深夜时序",
    };

    Some(format!("时序语境: {}", context))
}

/// Convert a day of week (0-6 or name) to cyclical context.
fn convert_day_of_week(day_str: &str) -> Option<String> {
    let is_weekend = match day_str.trim().to_lowercase().as_str() {
        "0" | "6" | "saturday" | "sunday" | "周六" | "周日" => true,
        "1" | "2" | "3" | "4" | "5" | "monday" | "tuesday" | "wednesday"
        | "thursday" | "friday" | "周一" | "周二" | "周三" | "周四" | "周五" => false,
        _ => return None,
    };

    let context = if is_weekend { "周末节律" } else { "工作日节律" };
    Some(format!("节律语境: {}", context))
}

/// Convert a weather condition string to atmospheric context.
fn convert_weather(weather_str: &str) -> Option<String> {
    let lower = weather_str.trim().to_lowercase();
    let context = if lower.contains("sun") || lower.contains("clear") || lower.contains("晴") {
        "晴朗"
    } else if lower.contains("cloud") || lower.contains("overcast") || lower.contains("阴") {
        "阴沉"
    } else if lower.contains("rain") || lower.contains("雨") {
        "雨天"
    } else if lower.contains("snow") || lower.contains("雪") {
        "雪天"
    } else if lower.contains("storm") || lower.contains("暴") {
        "风暴"
    } else if lower.contains("fog") || lower.contains("雾") {
        "雾气"
    } else {
        return None;
    };

    Some(format!("环境语境: {}", context))
}

/// Convert a season string to temporal context.
fn convert_season(season_str: &str) -> Option<String> {
    let lower = season_str.trim().to_lowercase();
    let context = match lower.as_str() {
        "spring" | "春" => "春",
        "summer" | "夏" => "夏",
        "autumn" | "fall" | "秋" => "秋",
        "winter" | "冬" => "冬",
        _ => return None,
    };

    Some(format!("季节语境: {}", context))
}

// ─── Basis Variation Generation ────────────────────────────────────────

/// Generate measurement basis text variations from environmental signals.
///
/// Each variation combines the base problem with semantic context from
/// one or more environmental signals. These variations become different
/// "measurement bases" (prompt variants) for multi-angle restoration.
///
/// The key insight: different environmental signals create genuinely
/// different measurement bases, producing true randomness in the
/// restoration results (different phrasing, emphasis, blind spots).
///
/// # Arguments
/// * `base_problem` — The judgment problem being explored.
/// * `signals` — Environmental signals to convert to semantic context.
///
/// # Returns
/// A list of basis text variations. Each is a unique combination of
/// the base problem with one or more semantic contexts.
pub fn generate_basis_variations(
    base_problem: &str,
    signals: &[EnvironmentalSignal],
) -> Vec<String> {
    // Convert all signals to semantic contexts
    let contexts: Vec<String> = signals
        .iter()
        .filter_map(|s| s.to_semantic_context())
        .collect();

    if contexts.is_empty() {
        return vec![base_problem.to_string()];
    }

    // Generate individual variations (one signal per variation)
    let mut variations: Vec<String> = contexts
        .iter()
        .map(|ctx| format!("{} [{}]", base_problem, ctx))
        .collect();

    // Also generate a combined variation (all signals together)
    if contexts.len() > 1 {
        let combined = contexts.join(" | ");
        variations.push(format!("{} [{}]", base_problem, combined));
    }

    // Always include the base problem without any context
    variations.push(base_problem.to_string());

    variations
}

/// Generate evenly spaced angles for a given number of measurements.
///
/// Used when the caller needs angle assignments for basis variations
/// but has no prior knowledge of the optimal angles.
pub fn generate_angles(count: usize) -> Vec<Angle> {
    if count == 0 {
        return vec![];
    }
    if count == 1 {
        return vec![Angle::from_degrees(0.0)];
    }

    (0..count)
        .map(|i| Angle::from_degrees(360.0 * i as f64 / count as f64))
        .collect()
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_of_day_conversion() {
        let signal = EnvironmentalSignal::new(SignalType::TimeOfDay, "08:30");
        let ctx = signal.to_semantic_context().unwrap();
        assert!(ctx.contains("清晨"));

        let signal = EnvironmentalSignal::new(SignalType::TimeOfDay, "14:00");
        let ctx = signal.to_semantic_context().unwrap();
        assert!(ctx.contains("午后"));
    }

    #[test]
    fn day_of_week_conversion() {
        let weekend = EnvironmentalSignal::new(SignalType::DayOfWeek, "6");
        assert!(weekend.to_semantic_context().unwrap().contains("周末"));

        let weekday = EnvironmentalSignal::new(SignalType::DayOfWeek, "3");
        assert!(weekday.to_semantic_context().unwrap().contains("工作日"));
    }

    #[test]
    fn weather_conversion() {
        let sunny = EnvironmentalSignal::new(SignalType::Weather, "sunny");
        assert!(sunny.to_semantic_context().unwrap().contains("晴朗"));

        let rainy = EnvironmentalSignal::new(SignalType::Weather, "大雨");
        assert!(rainy.to_semantic_context().unwrap().contains("雨天"));
    }

    #[test]
    fn season_conversion() {
        let spring = EnvironmentalSignal::new(SignalType::Season, "spring");
        assert!(spring.to_semantic_context().unwrap().contains("春"));
    }

    #[test]
    fn user_tag_conversion() {
        let tag = EnvironmentalSignal::new(SignalType::UserTag, "战略分析");
        assert!(tag.to_semantic_context().unwrap().contains("用户语境"));
    }

    #[test]
    fn invalid_signal_returns_none() {
        let invalid = EnvironmentalSignal::new(SignalType::DayOfWeek, "invalid");
        assert!(invalid.to_semantic_context().is_none());
    }

    #[test]
    fn empty_user_tag_returns_none() {
        let empty = EnvironmentalSignal::new(SignalType::UserTag, "");
        assert!(empty.to_semantic_context().is_none());
    }

    #[test]
    fn basis_variations_include_base_problem() {
        let signals = vec![
            EnvironmentalSignal::new(SignalType::TimeOfDay, "10:00"),
            EnvironmentalSignal::new(SignalType::Weather, "晴"),
        ];
        let variations = generate_basis_variations("test problem", &signals);

        // Should include: 2 individual + 1 combined + 1 base = 4
        assert_eq!(variations.len(), 4);
        assert!(variations.iter().any(|v| v == "test problem"));
        assert!(variations.iter().any(|v| v.contains("时序语境")));
        assert!(variations.iter().any(|v| v.contains("环境语境")));
    }

    #[test]
    fn basis_variations_no_signals() {
        let variations = generate_basis_variations("test problem", &[]);
        assert_eq!(variations.len(), 1);
        assert_eq!(variations[0], "test problem");
    }

    #[test]
    fn generate_angles_evenly_spaced() {
        let angles = generate_angles(4);
        assert_eq!(angles.len(), 4);
        // Should be 0, 90, 180, 270
        assert!((angles[0].degrees - 0.0).abs() < 1e-6);
        assert!((angles[1].degrees - 90.0).abs() < 1e-6);
        assert!((angles[2].degrees - 180.0).abs() < 1e-6);
        assert!((angles[3].degrees - 270.0).abs() < 1e-6);
    }

    #[test]
    fn generate_angles_empty() {
        assert!(generate_angles(0).is_empty());
    }

    #[test]
    fn generate_angles_single() {
        let angles = generate_angles(1);
        assert_eq!(angles.len(), 1);
        assert!((angles[0].degrees - 0.0).abs() < 1e-6);
    }
}
