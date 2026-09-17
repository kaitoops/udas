//! Adaptive Precision Control — four-gap closed-loop system.
//!
//! Designed 2026-07-24 to address precision gaps in UDAS:
//!
//! - **gap_1**: Hit detection — CR + τ_transit(N) → search→collapse transition
//! - **gap_2**: Dynamic K — growth_rate three-path budget adjustment
//! - **gap_3**: Precision-cost tradeoff — HIGH/MEDIUM/LOW tier with budget constraint
//! - **gap_4**: Adaptive step size — sigmoid-modulated gradient descent steps
//!
//! All thresholds are directional (not precise), emergent (not preset),
//! consistent with UDAS's anti-Bayesian design principle.
//!
//! 2026-07-25: Switched from `DensityField` to `InterferenceField` to
//! support the interference-based collapse model.

use crate::interference::{InterferenceField, contrast_ratio};
use crate::types::{GrowthPath, PrecisionTier};

/// Growth rate tracking for dynamic K (gap_2).
pub struct GrowthTracker {
    /// Recent contrast ratios (sliding window of 3).
    pub recent_cr: Vec<f64>,
    /// Historical CR values for quartile computation.
    pub cr_history: Vec<f64>,
    /// Window size.
    pub window: usize,
}

impl GrowthTracker {
    pub fn new(window_size: usize) -> Self {
        Self {
            recent_cr: Vec::with_capacity(window_size),
            cr_history: Vec::new(),
            window: window_size,
        }
    }

    /// Record a new CR value and compute growth rate.
    pub fn record(&mut self, cr: f64) -> Option<f64> {
        self.cr_history.push(cr);
        self.recent_cr.push(cr);
        if self.recent_cr.len() > self.window {
            self.recent_cr.remove(0);
        }
        self.compute_growth_rate()
    }

    /// Growth rate = median of (CR_t - CR_{t-1}) / Δt in sliding window.
    fn compute_growth_rate(&self) -> Option<f64> {
        if self.recent_cr.len() < 2 {
            return None;
        }
        let diffs: Vec<f64> = self.recent_cr.windows(2).map(|w| w[1] - w[0]).collect();
        let mut sorted = diffs.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some(sorted[sorted.len() / 2])
    }

    /// Current growth rate (most recent).
    pub fn growth_rate(&self) -> Option<f64> {
        self.compute_growth_rate()
    }

    /// Upper quartile (Q3) of historical CR values — growth_high threshold.
    pub fn growth_high(&self) -> Option<f64> {
        if self.cr_history.len() < 4 {
            return None;
        }
        let mut sorted = self.cr_history.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = (sorted.len() * 3) / 4;
        Some(sorted[idx])
    }

    /// Lower quartile (Q1) of historical CR values — growth_low threshold.
    pub fn growth_low(&self) -> Option<f64> {
        if self.cr_history.len() < 4 {
            return None;
        }
        let mut sorted = self.cr_history.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = sorted.len() / 4;
        Some(sorted[idx])
    }
}

/// Determine K growth path (gap_2).
pub fn classify_growth_path(tracker: &GrowthTracker) -> GrowthPath {
    let rate = match tracker.growth_rate() {
        Some(r) => r,
        None => return GrowthPath::NormalSearch,
    };
    let high = tracker.growth_high();
    let low = tracker.growth_low();

    match (high, low) {
        (Some(h), _) if rate > h => GrowthPath::FastHit,
        (_, Some(l)) if rate < l => GrowthPath::Stagnation,
        _ => GrowthPath::NormalSearch,
    }
}

/// Effective K based on growth path (gap_2).
///
/// - FastHit: K_effective = current_round + 1 (immediate transition)
/// - NormalSearch: K_effective = K_max (continue bisection)
/// - Stagnation: K_effective = current_round (premature stop)
pub fn effective_k(path: &GrowthPath, current_round: usize, k_max: usize) -> usize {
    match path {
        GrowthPath::FastHit => (current_round + 1).min(k_max),
        GrowthPath::NormalSearch => k_max,
        GrowthPath::Stagnation => current_round,
    }
}

// ─── gap_3: Precision-Cost Tradeoff ───────────────────────────────────────

/// Budget manager for precision tier control.
pub struct BudgetManager {
    /// Total token budget.
    pub total: usize,
    /// Remaining token budget.
    pub remaining: usize,
    /// Cost per single restoration (LLM call).
    pub cost_per_restoration: usize,
    /// Current precision tier.
    pub tier: PrecisionTier,
}

impl BudgetManager {
    pub fn new(total_budget: usize, cost_per_restore: usize) -> Self {
        Self {
            total: total_budget,
            remaining: total_budget,
            cost_per_restoration: cost_per_restore,
            tier: PrecisionTier::High,
        }
    }

    /// Consume budget for one restoration round.
    pub fn spend(&mut self, rounds: usize) {
        self.remaining = self
            .remaining
            .saturating_sub(rounds * self.cost_per_restoration);
    }

    /// Check downgrade triggers and apply if needed.
    pub fn check_downgrade(
        &mut self,
        field: &InterferenceField,
        resolution: usize,
    ) -> PrecisionTier {
        let budget_threshold = (self.total as f64 * 0.3) as usize;
        let cr = contrast_ratio(field, resolution);
        let tau_downgrade = field.tau_base * 2.0; // 4.0

        // Trigger 1: Budget exhaustion
        if self.remaining < budget_threshold && self.tier != PrecisionTier::Low {
            self.tier = match self.tier {
                PrecisionTier::High => PrecisionTier::Medium,
                _ => PrecisionTier::Low,
            };
        }

        // Trigger 2: Structure clarity allows downgrade
        if cr > tau_downgrade && self.tier == PrecisionTier::High {
            self.tier = PrecisionTier::Medium;
        }

        // Trigger 3: Hard floor
        if self.remaining < 3 * self.cost_per_restoration {
            self.tier = PrecisionTier::Low;
        }

        self.tier
    }
}

/// Effective rounds based on precision tier and K_max.
pub fn tier_rounds(tier: PrecisionTier, k_max: usize) -> usize {
    match tier {
        PrecisionTier::High => k_max,
        PrecisionTier::Medium => k_max / 2,
        PrecisionTier::Low => 3, // cold-start minimum
    }
}

// ─── gap_4: Adaptive Step Size ────────────────────────────────────────────

/// Sigmoid-modulated step size for gradient descent.
///
/// α_t = α₀ · (1 - t/K) · σ(|dI/dθ|_t)
/// where σ(x) = 1 / (1 + exp(-(x - γ) / γ_scale))
///
/// - Steep gradient (|dI/dθ| >> γ): σ ≈ 1 → normal decay
/// - Flat gradient (|dI/dθ| << γ): σ ≈ 0 → step clamped (avoid wasting iterations)
pub fn adaptive_step_size(
    alpha_0: f64,
    t: usize,
    k: usize,
    gradient_magnitude: f64,
    gamma: f64,
) -> f64 {
    let time_decay = 1.0 - (t as f64 / k as f64);
    let gamma_scale = gamma / 4.0;
    let sigmoid = 1.0 / (1.0 + (-(gradient_magnitude - gamma) / gamma_scale).exp());
    alpha_0 * time_decay * sigmoid
}

/// Compute the current gamma (median gradient magnitude of observed gradients).
pub fn compute_gamma(observed_gradients: &[f64]) -> Option<f64> {
    if observed_gradients.is_empty() {
        return None;
    }
    let mut sorted = observed_gradients.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(sorted[sorted.len() / 2])
}

// ─── Integrated Pipeline Orchestrator ─────────────────────────────────────

/// Central orchestration: run one full UDAS precision control cycle.
///
/// Called after each restoration round. Checks gap_1 (transition),
/// gap_2 (K adjustment), gap_3 (budget), gap_4 (step size).
pub struct PrecisionController {
    pub growth_tracker: GrowthTracker,
    pub budget: BudgetManager,
    pub observed_gradients: Vec<f64>,
}

impl PrecisionController {
    pub fn new(total_budget: usize, cost_per_restore: usize, window: usize) -> Self {
        Self {
            growth_tracker: GrowthTracker::new(window),
            budget: BudgetManager::new(total_budget, cost_per_restore),
            observed_gradients: Vec::new(),
        }
    }

    /// Record a restoration round and update all precision state.
    /// Returns the recommended next action.
    pub fn tick(
        &mut self,
        field: &InterferenceField,
        gradient_mag: Option<f64>,
        resolution: usize,
        current_round: usize,
        k_max: usize,
    ) -> PrecisionAction {
        // gap_1: Check transition condition
        let should_switch = crate::interference::should_transition(field, resolution);

        // gap_2: Record CR and update growth tracker
        let cr = contrast_ratio(field, resolution);
        self.growth_tracker.record(cr);
        let path = classify_growth_path(&self.growth_tracker);
        let k_eff = effective_k(&path, current_round, k_max);

        // gap_3: Check budget downgrade
        let tier = self.budget.check_downgrade(field, resolution);
        let rounds = tier_rounds(tier, k_max);

        // gap_4: Record gradient for adaptive step size
        if let Some(mag) = gradient_mag {
            self.observed_gradients.push(mag);
        }
        let gamma = compute_gamma(&self.observed_gradients);
        let step_size = gamma.map(|g| {
            adaptive_step_size(
                std::f64::consts::PI / 8.0,
                current_round,
                k_eff,
                gradient_mag.unwrap_or(0.0),
                g,
            )
        });

        PrecisionAction {
            should_transition: should_switch,
            k_effective: k_eff,
            tier,
            remaining_rounds: rounds,
            growth_path: path,
            step_size,
            gamma,
        }
    }
}

/// Action recommendation after a precision control tick.
pub struct PrecisionAction {
    /// Should we switch from bisection to gradient descent?
    pub should_transition: bool,
    /// Effective K for remaining search.
    pub k_effective: usize,
    /// Current precision tier.
    pub tier: PrecisionTier,
    /// Remaining rounds in current tier.
    pub remaining_rounds: usize,
    /// Current growth classification.
    pub growth_path: GrowthPath,
    /// Adaptive step size for gradient descent (None = not yet computed).
    pub step_size: Option<f64>,
    /// Current gamma (median gradient magnitude).
    pub gamma: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_is_symmetric() {
        let s1 = 1.0 / (1.0 + (-1.0_f64).exp());
        let s2 = 1.0 / (1.0 + (1.0_f64).exp());
        assert!((s1 + s2 - 1.0).abs() < 0.001);
    }

    #[test]
    fn adaptive_step_decreases_with_flat_gradient() {
        let step_steep = adaptive_step_size(1.0, 5, 20, 10.0, 1.0);
        let step_flat = adaptive_step_size(1.0, 5, 20, 0.01, 1.0);
        assert!(
            step_steep > step_flat,
            "steep gradient should yield larger step"
        );
    }

    #[test]
    fn stagnation_path_reduces_k() {
        let mut tracker = GrowthTracker::new(3);
        // Simulate stagnating CR values
        tracker.cr_history = vec![3.0, 3.0, 3.0, 3.0, 2.9, 2.9, 2.9];
        tracker.record(2.8);
        tracker.record(2.8);
        let path = classify_growth_path(&tracker);
        // Growth rate ≈ 0 < growth_low = Q1 ≈ 2.9 → Stagnation
        assert!(matches!(path, GrowthPath::Stagnation));
    }

    #[test]
    fn tier_rounds_correct_mapping() {
        assert_eq!(tier_rounds(PrecisionTier::High, 8), 8);
        assert_eq!(tier_rounds(PrecisionTier::Medium, 8), 4);
        assert_eq!(tier_rounds(PrecisionTier::Low, 8), 3);
    }
}
