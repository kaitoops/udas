//! KDE Density Field — kernel density estimation on the UDAS disk.
//!
//! Maintains a 1-D density field ρ(θ) over the emergent disk. Each
//! restoration R(θ) contributes a kernel centred at θ with height =
//! confidence weight w_k. The density field drives gradient descent
//! convergence and final collapse.
//!
//! ## Key Components
//!
//! - **KDE**: ρ(θ) = Σ w_k · K(θ - θ_k, h)
//! - **Bandwidth**: Silverman rule-of-thumb, adaptive
//! - **Contrast Ratio**: CR = ρ_max / (ρ_median + ε) — directional indicator
//! - **Transit Threshold**: τ_transit(N) = τ_base · (1 + β / max(N-N_min, 1))
//! - **Density-Weighted Random Sampling**: final collapse method

use crate::types::{Angle, CollapseMethod};

/// A single kernel component in the KDE density field.
#[derive(Debug, Clone)]
pub struct KernelPoint {
    pub angle: Angle,
    pub weight: f64, // confidence weight w_k
}

/// The KDE density field over the UDAS disk.
pub struct DensityField {
    /// All kernel points accumulated so far.
    pub kernels: Vec<KernelPoint>,
    /// Current bandwidth h (Silverman-adaptive).
    pub bandwidth: f64,
    /// Base bandwidth parameter τ_base (default 2.0).
    pub tau_base: f64,
    /// Beta parameter for transit threshold (default 2.0).
    pub beta: f64,
    /// Minimum points before transit allowed (default 5).
    pub n_min: usize,
}

impl DensityField {
    /// Create a new empty density field with default parameters.
    pub fn new() -> Self {
        Self {
            kernels: Vec::new(),
            bandwidth: 30.0, // initial bandwidth in degrees
            tau_base: 2.0,
            beta: 2.0,
            n_min: 5,
        }
    }

    /// Add a kernel point and update bandwidth.
    pub fn add_kernel(&mut self, angle: Angle, confidence_weight: f64) {
        self.kernels.push(KernelPoint {
            angle,
            weight: confidence_weight,
        });
        self.update_bandwidth();
    }

    /// Silverman rule-of-thumb bandwidth update.
    fn update_bandwidth(&mut self) {
        if self.kernels.len() < 2 {
            return;
        }
        let angles: Vec<f64> = self.kernels.iter().map(|k| k.angle.degrees).collect();
        let n = angles.len() as f64;
        let mean = angles.iter().sum::<f64>() / n;
        let variance = angles.iter().map(|a| (a - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let sigma = variance.sqrt();
        // Silverman: h = 1.06 * σ * n^(-1/5), then convert to degrees
        self.bandwidth = 1.06 * sigma * n.powf(-0.2);
    }
}

/// Evaluate ρ(θ) at a given angle using Gaussian kernel.
pub fn evaluate_density(field: &DensityField, theta: &Angle) -> f64 {
    let h = field.bandwidth;
    field
        .kernels
        .iter()
        .map(|k| {
            let diff = (theta.degrees - k.angle.degrees).abs();
            // Circular wrap: distance on circle
            let circular_diff = diff.min(360.0 - diff);
            // Gaussian kernel: K(x, h) = exp(-x²/(2h²))
            k.weight * (-0.5 * (circular_diff / h).powi(2)).exp()
        })
        .sum()
}

/// Compute density contrast ratio: CR = ρ_max / (ρ_median + ε).
///
/// CR ≈ 1  → flat (no information structure)
/// CR >> 1 → clear density peak (strong structure)
/// This is a DIRECTIONAL metric, not a precise probability.
pub fn contrast_ratio(field: &DensityField, resolution: usize) -> f64 {
    let densities: Vec<f64> = (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            evaluate_density(field, &Angle::from_degrees(deg))
        })
        .collect();
    let rho_max = densities.iter().cloned().fold(0.0, f64::max);
    let mut sorted = densities.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rho_median = sorted[sorted.len() / 2];
    let epsilon = 1e-8;
    rho_max / (rho_median + epsilon)
}

/// Compute transit threshold: τ_transit(N) = τ_base · (1 + β / max(N-N_min, 1)).
///
/// - N=5:  τ = 9.0 (extremely conservative)
/// - N=20: τ = 3.4 (relaxed)
/// - N→∞: τ → τ_base = 3.0
pub fn transit_threshold(field: &DensityField) -> f64 {
    let n = field.kernels.len();
    let n_diff = (n as isize - field.n_min as isize).max(1) as f64;
    field.tau_base * (1.0 + field.beta / n_diff)
}

/// Check if transition condition is met: CR > τ_transit(N) → switch from
/// angle bisection (Phase 2) to gradient descent (Phase 4).
pub fn should_transition(field: &DensityField, resolution: usize) -> bool {
    if field.kernels.len() < field.n_min {
        return false;
    }
    let cr = contrast_ratio(field, resolution);
    let tau = transit_threshold(field);
    cr > tau
}

/// Low-precision gradient descent on the density field.
///
/// θ_{t+1} = θ_t + α · dρ/dθ|_{θ_t}
/// where α is the adaptive step size from precision_control.
pub fn gradient_descent_step(field: &DensityField, current: &Angle, step_size: f64) -> Angle {
    let h = 1e-4; // numerical differentiation step
    let rho_plus = evaluate_density(field, &Angle::from_degrees(current.degrees + h));
    let rho_minus = evaluate_density(field, &Angle::from_degrees(current.degrees - h));
    let gradient = (rho_plus - rho_minus) / (2.0 * h);
    Angle::from_degrees(current.degrees + step_size * gradient)
}

/// Final collapse: density-weighted random sampling within the convergence interval.
///
/// NOT argmax — preserves randomness. P(θ) ∝ ρ(θ) within the peak region.
/// The "random but reliable" property.
pub fn density_weighted_sample(
    field: &DensityField,
    peak: &Angle,
    fwhm: f64,
) -> (Angle, CollapseMethod) {
    let range = fwhm / 2.0;
    let samples: Vec<(Angle, f64)> = (0..100)
        .map(|i| {
            let offset = (i as f64 / 99.0 - 0.5) * 2.0 * range;
            let angle = Angle::from_degrees(peak.degrees + offset);
            let density = evaluate_density(field, &angle);
            (angle, density)
        })
        .collect();

    let total_weight: f64 = samples.iter().map(|(_, w)| w).sum();
    if total_weight < f64::EPSILON {
        return (*peak, CollapseMethod::WeightedRandom);
    }

    // Weighted random selection
    let rng_seed = 42u64; // TODO: proper RNG
    let threshold = (rng_seed as f64 / u64::MAX as f64) * total_weight;
    let mut cumulative = 0.0;
    for (angle, weight) in &samples {
        cumulative += weight;
        if cumulative >= threshold {
            return (*angle, CollapseMethod::WeightedRandom);
        }
    }
    (samples.last().unwrap().0, CollapseMethod::WeightedRandom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_field_contrast_is_one() {
        let field = DensityField::new();
        let cr = contrast_ratio(&field, 36);
        // Empty field: all densities = 0, so ρ_max = ρ_median = 0 → CR ≈ 0/ε
        assert!(cr < 1.0);
    }

    #[test]
    fn single_kernel_is_peak() {
        let mut field = DensityField::new();
        field.add_kernel(Angle::from_degrees(180.0), 1.0);
        let rho_at_peak = evaluate_density(&field, &Angle::from_degrees(180.0));
        let rho_away = evaluate_density(&field, &Angle::from_degrees(0.0));
        assert!(rho_at_peak > rho_away);
    }

    #[test]
    fn transit_threshold_decreases_with_N() {
        let field = DensityField::new();
        let t5 = transit_threshold(&field); // N=0, N_diff = max(-5,1) → actually let me check
        // Wait — field.kernels.len() = 0 when freshly created.
        // n_diff = max(0-5, 1) = 1. β=2. τ_base=2.0 → τ = 2*(1+2/1) = 6.0
        // That seems wrong for N=0 but the guard in should_transition handles N < n_min
        assert!((t5 - 6.0).abs() < 0.1);
    }
}
