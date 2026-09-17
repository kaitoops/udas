//! Circuit Breaker — coherent-error guard (W5).
//!
//! Detects "information-starved" searches by combining the existing slope-based
//! stagnation signal with a plateau-CR floor. Only when BOTH signal that the
//! field is flat **and** low does the breaker trip → premature termination +
//! upgrade to human (= pilot anchor).
//!
//! ## Design (W5.1, G3-approved)
//!
//! Trigger = **A ∧ B**:
//! - **A (slope)** `growth_path == Stagnation` sustained ≥ `w_trigger` rounds.
//! - **B (level)** plateau-CR median over the recent window `< plateau_floor`.
//!
//! The level floor is the key revision over a pure slope criterion. W4 measured
//! that a *converged high plateau* (final_cr ≈ 3.5) and a *truly divergent flat
//! field* (final_cr ≈ 1.7) are indistinguishable by slope (both → 0 difference),
//! so slope alone would false-trip 6/8 convergent questions. The floor separates
//! them on CR level instead.
//!
//! ## Semantics
//!
//! Wraps the existing stagnation path **without changing its logic**. It only
//! decides *whether to escalate*. The default install is `None` → an unscanned
//! engine keeps the pre-W5 behavior, so the shipped baseline is unaffected.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::types::GrowthPath;
use serde::Serialize;

/// Plateau-CR floor separating a low (divergent) from a high (converged)
/// plateau. W4 measured diverge ≈ 1.7 vs converge ≈ 3.5; 2.5 sits cleanly in
/// the gap, sensitivity [2.0, 3.0].
pub const DEFAULT_PLATEAU_FLOOR: f64 = 2.5;

/// Rounds the stagnation signal must persist before a trip is considered.
pub const DEFAULT_W_TRIGGER: usize = 3;

/// Breaker decision for one evaluation round / after a human release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripStatus {
    /// Not stalled, or stalled but above the plateau floor → keep searching.
    Idle,
    /// Stalled **and** below the floor → stop search, escalate to human.
    Tripped,
    /// Was tripped, then a human released it → resume searching.
    Recovered,
}

/// Structured breaker event, serialized to `calibration/breakers/` logs.
#[derive(Debug, Clone, Serialize)]
pub struct BreakerEvent {
    /// Question identifier if known (set by the caller/harness).
    pub question_id: Option<String>,
    /// Machine-readable reason.
    pub reason: String,
    /// Round at which the breaker tripped.
    pub rounds: usize,
    /// Growth rate at trip time (None if a window was not yet available).
    pub growth_rate: Option<f64>,
    /// Growth-rate sliding window size in use.
    pub window: usize,
    /// Plateau-CR median at trip time.
    pub plateau_cr_median: f64,
    /// Plateau floor the median was compared against.
    pub plateau_floor: f64,
    /// Contrast ratio of the field at trip time.
    pub final_cr: f64,
}

impl BreakerEvent {
    /// Append this event to `calibration/breakers/{ts}-{question_id}.jsonl`.
    ///
    /// Creates the `breakers/` directory if missing and writes one JSON line
    /// (append-only), so the file accumulates a machine audit trail. Returns the
    /// path written. The caller (harness / orchestrator) supplies the
    /// calibration directory and, when known, fills `question_id` beforehand.
    pub fn write_to_disk(&self, calibration_dir: &Path) -> io::Result<PathBuf> {
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string();
        let qid = self.question_id.as_deref().unwrap_or("unknown");
        let dir = calibration_dir.join("breakers");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{ts}-{qid}.jsonl"));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{}", serde_json::to_string(self)?)?;
        Ok(path)
    }
}

/// The coherent-error gate itself.
pub struct CircuitBreaker {
    plateau_floor: f64,
    w_trigger: usize,
    window: usize,
    consecutive_stagnation: usize,
    recent_cr: Vec<f64>,
    last_plateau_median: f64,
    tripped: bool,
}

impl CircuitBreaker {
    /// Create the breaker with the shipped defaults (floor 2.5, w_trigger 3,
    /// window 3). Injecting non-defaults never mutates the defaults.
    pub fn new() -> Self {
        Self {
            plateau_floor: DEFAULT_PLATEAU_FLOOR,
            w_trigger: DEFAULT_W_TRIGGER,
            window: 3,
            consecutive_stagnation: 0,
            recent_cr: Vec::new(),
            last_plateau_median: 0.0,
            tripped: false,
        }
    }

    /// Set the plateau-CR floor (criterion B threshold).
    pub fn with_plateau_floor(mut self, floor: f64) -> Self {
        self.plateau_floor = floor;
        self
    }

    /// Set how many consecutive stagnation rounds must pass before a trip.
    pub fn with_w_trigger(mut self, w: usize) -> Self {
        self.w_trigger = w;
        self
    }

    /// Set the median window used for the plateau-CR estimate.
    pub fn with_window(mut self, window: usize) -> Self {
        self.window = window.max(1);
        self
    }

    /// Whether the breaker has already tripped in this session.
    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    /// Last plateau-CR median computed by `evaluate`.
    pub fn last_plateau_median(&self) -> f64 {
        self.last_plateau_median
    }

    /// Feed one round's CR and growth-path classification.
    ///
    /// The engine calls this after each completed active-search round. Returns
    /// `Tripped` on the round that first confirms A ∧ B; afterwards it stays in
    /// a tripped state until a human `release()`.
    pub fn evaluate(&mut self, cr: f64, growth_path: &GrowthPath) -> TripStatus {
        // Maintain the recent-CR ring used to estimate the plateau level.
        self.recent_cr.push(cr);
        if self.recent_cr.len() > self.window {
            self.recent_cr.remove(0);
        }
        self.last_plateau_median = median(&self.recent_cr);

        if matches!(growth_path, GrowthPath::Stagnation) {
            self.consecutive_stagnation += 1;
        } else {
            self.consecutive_stagnation = 0;
        }

        if self.tripped {
            return TripStatus::Tripped;
        }

        // A ∧ B
        let slope_stalled = self.consecutive_stagnation >= self.w_trigger;
        let level_low = self.last_plateau_median < self.plateau_floor;
        if slope_stalled && level_low {
            self.tripped = true;
            return TripStatus::Tripped;
        }

        TripStatus::Idle
    }

    /// Human release point (采纳 / 拒绝 / 修正 → 放行继续搜索).
    pub fn release(&mut self) -> TripStatus {
        self.tripped = false;
        self.consecutive_stagnation = 0;
        TripStatus::Recovered
    }
}

/// Median of a slice; returns 0.0 for an empty slice.
fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[s.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trip_requires_stalled_low_plateau() {
        // Divergent-ish: stagnation persists below the floor → trips on the
        // round that first reaches w_trigger=3 consecutive stagnant samples.
        let mut br = CircuitBreaker::new();
        assert_eq!(br.evaluate(1.5, &GrowthPath::Stagnation), TripStatus::Idle);
        assert_eq!(br.evaluate(1.5, &GrowthPath::Stagnation), TripStatus::Idle);
        assert_eq!(br.evaluate(1.5, &GrowthPath::Stagnation), TripStatus::Tripped);
        assert!(br.is_tripped());
    }

    #[test]
    fn high_plateau_is_rescued_even_when_stalled() {
        // Convergent-ish: stagnation with a high plateau (3.5 ≥ floor) never trips.
        let mut br = CircuitBreaker::new();
        let mut tripped = false;
        for _ in 0..12 {
            if br.evaluate(3.5, &GrowthPath::Stagnation) == TripStatus::Tripped {
                tripped = true;
            }
        }
        assert!(!tripped, "high plateau must be rescued by the floor guard");
        assert!(!br.is_tripped(), "high plateau must never trip");
    }

    #[test]
    fn non_stagnation_never_trips() {
        let mut br = CircuitBreaker::new();
        let mut tripped = false;
        for _ in 0..12 {
            if br.evaluate(1.5, &GrowthPath::NormalSearch) == TripStatus::Tripped {
                tripped = true;
            }
        }
        assert!(!tripped, "growth path not Stagnation → never trip");
    }

    #[test]
    fn release_recovers_and_allows_resume() {
        // Trigger, then human release → Recovered → can resume to Idle.
        let mut br = CircuitBreaker::new();
        br.evaluate(1.5, &GrowthPath::Stagnation);
        br.evaluate(1.5, &GrowthPath::Stagnation);
        assert_eq!(br.evaluate(1.5, &GrowthPath::Stagnation), TripStatus::Tripped);
        assert_eq!(br.release(), TripStatus::Recovered);
        assert!(!br.is_tripped(), "after release must not be tripped");
        let s = br.evaluate(1.5, &GrowthPath::NormalSearch);
        assert_eq!(s, TripStatus::Idle, "released breaker resumes normally");
    }
}