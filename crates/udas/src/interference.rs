//! Interference Pattern Field — quantum-inspired interference model
//! that replaces the additive KDE density field.
//!
//! ## Mathematical Structure (from UDAS-INTERFERENCE-ARCHITECTURE.md §4)
//!
//! **Amplitude**:    αₖ = √cₖ · e^{iφₖ}
//! **Phase**:        cos(φₖ - φⱼ) = cosine_sim(bₖ, bⱼ)
//! **Pattern**:      I = |Σ αₖ|² = Σ|αₖ|² + Σₖ≠ⱼ αₖαⱼ* cos(φₖ-φⱼ)
//! **Gated**:        I_cross(k,j) = αₖαⱼ · cos(φₖ-φⱼ) · sgn(⟨eₖ|eⱼ⟩ - τ)
//! **Angular**:      I(θ) = Σₖ |αₖ|² K(θ-θₖ)
//!                       + Σₖ<ⱼ 2·√(cₖcⱼ)·cos_sim(bₖ,bⱼ)·sgn(cos_sim(eₖ,eⱼ)-τ)·K(θ-θₖⱼ)
//!
//! - Independent term (Σ|αₖ|²): the old additive KDE model
//! - Cross term: NEW interference mechanism = source of new information
//! - Constructive interference (cross > 0): consensus between measurements
//! - Destructive interference (cross < 0): contradiction exposed
//!
//! ## Key Difference from density.rs
//!
//! density.rs: ρ(θ) = Σ wₖ K(θ-θₖ)  — pure addition, no cross terms
//! interference.rs: I(θ) = independent + cross terms — interference produces
//! new information that the additive model cannot capture.

use crate::types::{Angle, CollapseMethod, Complex, Embedding};

// ─── Data Structures ────────────────────────────────────────────────────

/// A single measurement in the interference field.
///
/// Each measurement is one "projection" of the underlying superposition
/// state (LLM weights) via a specific measurement basis (prompt variant).
#[derive(Debug, Clone)]
pub struct Measurement {
    /// Angle on the UDAS disk where this measurement landed.
    pub angle: Angle,
    /// Confidence weight cₖ ∈ [0, 1] from the evidence ledger.
    pub confidence: f64,
    /// Embedding of the measurement basis (prompt variant text).
    /// Used to compute phase differences: cos(φₖ-φⱼ) = cosine_sim(bₖ, bⱼ).
    pub basis_embedding: Embedding,
    /// Embedding of the restoration result (evidence content).
    /// Used for consistency gating: sgn(cosine_sim(eₖ, eⱼ) - τ).
    pub result_embedding: Embedding,
    /// MDS-derived phase φₖ for the complex framework.
    /// None = real mode (backward compatible). Some(φ) = complex mode.
    pub phase: Option<f64>,
}

/// The interference pattern field over the UDAS disk.
///
/// Replaces `DensityField` with a model that includes cross-term
/// interference between measurement bases.
pub struct InterferenceField {
    /// All measurements accumulated so far.
    pub measurements: Vec<Measurement>,
    /// Current kernel bandwidth h (degrees), Silverman-adaptive.
    pub bandwidth: f64,
    /// Base bandwidth parameter τ_base (default 2.0).
    pub tau_base: f64,
    /// Beta parameter for transit threshold (default 2.0).
    pub beta: f64,
    /// Minimum measurements before transit allowed (default 5).
    pub n_min: usize,
    /// Consistency threshold τ for gating (default 0.5).
    pub consistency_threshold: f64,
}

impl InterferenceField {
    /// Create a new empty interference field with default parameters.
    pub fn new() -> Self {
        Self {
            measurements: Vec::new(),
            bandwidth: 30.0,
            tau_base: 2.0,
            beta: 2.0,
            n_min: 5,
            consistency_threshold: 0.5,
        }
    }

    /// Add a measurement and update bandwidth (real mode, backward compatible).
    pub fn add_measurement(
        &mut self,
        angle: Angle,
        confidence: f64,
        basis_embedding: Embedding,
        result_embedding: Embedding,
    ) {
        self.add_measurement_with_phase(angle, confidence, basis_embedding, result_embedding, None);
    }

    /// Add a measurement with an optional MDS-derived phase.
    ///
    /// When phase is Some(φ), the measurement participates in complex
    /// interference evaluation. When None, it falls back to real mode.
    pub fn add_measurement_with_phase(
        &mut self,
        angle: Angle,
        confidence: f64,
        basis_embedding: Embedding,
        result_embedding: Embedding,
        phase: Option<f64>,
    ) {
        self.measurements.push(Measurement {
            angle,
            confidence: confidence.clamp(0.0, 1.0),
            basis_embedding,
            result_embedding,
            phase,
        });
        self.update_bandwidth();
    }

    /// Set MDS-derived phases for all measurements.
    ///
    /// Call this after adding measurements to switch the field from
    /// real mode to complex mode. The phases should come from
    /// geometry::compute_mds_phases().
    pub fn set_phases(&mut self, phases: &[f64]) {
        for (i, &phi) in phases.iter().enumerate() {
            if i < self.measurements.len() {
                self.measurements[i].phase = Some(phi);
            }
        }
    }

    /// Check if all measurements have phases set (complex mode ready).
    pub fn has_phases(&self) -> bool {
        !self.measurements.is_empty() && self.measurements.iter().all(|m| m.phase.is_some())
    }

    /// Silverman rule-of-thumb bandwidth update (same as density.rs).
    fn update_bandwidth(&mut self) {
        if self.measurements.len() < 2 {
            return;
        }
        let angles: Vec<f64> = self.measurements.iter().map(|m| m.angle.degrees).collect();
        let n = angles.len() as f64;
        let mean = angles.iter().sum::<f64>() / n;
        let variance = angles.iter().map(|a| (a - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let sigma = variance.sqrt();
        self.bandwidth = 1.06 * sigma * n.powf(-0.2);
        // Clamp to sane range
        self.bandwidth = self.bandwidth.max(5.0).min(120.0);
    }

    /// Number of measurements stored.
    pub fn len(&self) -> usize {
        self.measurements.len()
    }

    /// Is the field empty?
    pub fn is_empty(&self) -> bool {
        self.measurements.is_empty()
    }
}

impl Default for InterferenceField {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Core Math Helpers ──────────────────────────────────────────────────

/// Cosine similarity between two embedding vectors.
///
/// Returns value in [-1, 1]. This directly gives cos(φₖ - φⱼ)
/// for measurement bases bₖ and bⱼ.
fn cosine_sim(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
        .max(f64::EPSILON);
    let norm_b: f64 = b
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
        .max(f64::EPSILON);
    dot / (norm_a * norm_b)
}

/// Gaussian kernel K(x, h) = exp(-x² / (2h²)).
fn gaussian_kernel(diff: f64, bandwidth: f64) -> f64 {
    if bandwidth < f64::EPSILON {
        return 0.0;
    }
    (-0.5 * (diff / bandwidth).powi(2)).exp()
}

/// Circular distance on [0°, 360°): shortest arc between two angles.
fn circular_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).abs().rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Midpoint of two angles on the circle (shortest arc midpoint).
fn circular_midpoint(a: f64, b: f64) -> f64 {
    let d = circular_diff(a, b);
    // Determine direction: go from a toward b along the shorter arc
    let diff_raw = (b - a).rem_euclid(360.0);
    if diff_raw <= 180.0 {
        (a + d / 2.0).rem_euclid(360.0)
    } else {
        (a - d / 2.0).rem_euclid(360.0)
    }
}

// ─── Interference Pattern Evaluation ────────────────────────────────────

/// Evaluate the interference pattern I(θ) at a given angle.
///
/// I(θ) = independent_term + cross_term
///
/// - Independent: Σₖ cₖ · K(θ - θₖ)  — same as old KDE
/// - Cross: Σₖ<ⱼ 2·√(cₖcⱼ)·cos_sim(bₖ,bⱼ)·sgn(cos_sim(eₖ,eⱼ)-τ)·K(θ-θₖⱼ)
///
/// The cross term is the NEW information source: it captures constructive
/// interference (consensus) and destructive interference (contradiction)
/// between measurement bases.
pub fn evaluate_pattern(field: &InterferenceField, theta: &Angle) -> f64 {
    // Dispatch to complex mode when MDS phases are set.
    // This makes ALL downstream functions (find_pattern_peak, compute_fwhm,
    // slerp_collapse, pattern_gradient, etc.) automatically support complex
    // mode without any additional changes.
    if field.has_phases() {
        return evaluate_complex_probability(field, theta);
    }
    evaluate_real_pattern(field, theta)
}

/// Real-mode interference pattern evaluation (original implementation).
fn evaluate_real_pattern(field: &InterferenceField, theta: &Angle) -> f64 {
    let n = field.measurements.len();
    if n == 0 {
        return 0.0;
    }

    let h = field.bandwidth;
    let tau = field.consistency_threshold;

    // ── Independent term: Σₖ cₖ · K(θ - θₖ) ──
    let independent: f64 = field
        .measurements
        .iter()
        .map(|m| {
            let diff = circular_diff(theta.degrees, m.angle.degrees);
            m.confidence * gaussian_kernel(diff, h)
        })
        .sum();

    // ── Cross term: Σₖ<ⱼ 2·√(cₖcⱼ)·cos_sim(bₖ,bⱼ)·sgn(sim(eₖ,eⱼ)-τ)·K(θ-θₖⱼ) ──
    let mut cross: f64 = 0.0;
    for i in 0..n {
        for j in (i + 1)..n {
            let mi = &field.measurements[i];
            let mj = &field.measurements[j];

            let alpha_ij = (mi.confidence * mj.confidence).sqrt();
            let cos_phase = cosine_sim(&mi.basis_embedding, &mj.basis_embedding);
            let result_sim = cosine_sim(&mi.result_embedding, &mj.result_embedding);
            let consistency_gate = if result_sim > tau { 1.0 } else { -1.0 };

            let mid = circular_midpoint(mi.angle.degrees, mj.angle.degrees);
            let diff = circular_diff(theta.degrees, mid);

            // Factor 2 because Σₖ≠ⱼ = 2·Σₖ<ⱼ (symmetric pairs)
            cross += 2.0 * alpha_ij * cos_phase * consistency_gate * gaussian_kernel(diff, h);
        }
    }

    independent + cross
}

/// Evaluate only the cross-term contribution at θ (for diagnostics).
///
/// Useful for visualising where interference is constructive vs destructive.
pub fn evaluate_cross_term(field: &InterferenceField, theta: &Angle) -> f64 {
    let n = field.measurements.len();
    if n < 2 {
        return 0.0;
    }

    let h = field.bandwidth;
    let tau = field.consistency_threshold;
    let mut cross: f64 = 0.0;

    for i in 0..n {
        for j in (i + 1)..n {
            let mi = &field.measurements[i];
            let mj = &field.measurements[j];

            let alpha_ij = (mi.confidence * mj.confidence).sqrt();
            let cos_phase = cosine_sim(&mi.basis_embedding, &mj.basis_embedding);
            let result_sim = cosine_sim(&mi.result_embedding, &mj.result_embedding);
            let consistency_gate = if result_sim > tau { 1.0 } else { -1.0 };

            let mid = circular_midpoint(mi.angle.degrees, mj.angle.degrees);
            let diff = circular_diff(theta.degrees, mid);

            cross += 2.0 * alpha_ij * cos_phase * consistency_gate * gaussian_kernel(diff, h);
        }
    }

    cross
}

// ─── Complex Interference Evaluation ────────────────────────────────────
//
// The complex framework evaluates the interference pattern as a complex
// amplitude ψ(θ) = Σₖ αₖ · K(θ-θₖ), where αₖ = √cₖ · e^{iφₖ}.
//
// The probability density is I(θ) = |ψ(θ)|².
// The imaginary component Im(ψ) captures phase information lost in real mode.
//
// Key difference from real evaluate_pattern():
// - Real: I(θ) = Σₖ cₖ·K(θ-θₖ) + Σₖ<ⱼ 2·√(cₖcⱼ)·cos(φₖ-φⱼ)·gate·K(θ-θₖⱼ)
// - Complex: I(θ) = |Σₖ √cₖ·e^{iφₖ}·K(θ-θₖ)|²
//           = Σₖ cₖ·K²(θ-θₖ) + Σₖ≠ⱼ √(cₖcⱼ)·cos(φₖ-φⱼ)·K(θ-θₖ)·K(θ-θⱼ)
//
// The complex formula naturally produces:
// 1. Product of kernels K(θ-θₖ)·K(θ-θⱼ) instead of kernel at midpoint
// 2. Full cos(φₖ-φⱼ) from MDS phases instead of cos_sim(basis_embeddings)
// 3. An imaginary component that signals unresolved phase structure

/// Evaluate the complex amplitude ψ(θ) = Σₖ αₖ · K(θ-θₖ).
///
/// Each measurement contributes a complex amplitude:
///   αₖ = √cₖ · e^{iφₖ}
/// where φₖ is the MDS-derived phase (or 0 if not set, falling back
/// to real mode behavior).
///
/// The kernel K(θ-θₖ) is real-valued, so it scales the amplitude
/// without rotating it.
pub fn evaluate_complex_amplitude(field: &InterferenceField, theta: &Angle) -> Complex {
    let n = field.measurements.len();
    if n == 0 {
        return Complex::new(0.0, 0.0);
    }

    let h = field.bandwidth;
    let mut psi = Complex::new(0.0, 0.0);

    for m in &field.measurements {
        let diff = circular_diff(theta.degrees, m.angle.degrees);
        let kernel = gaussian_kernel(diff, h);
        let phi = m.phase.unwrap_or(0.0);
        let amplitude = Complex::from_polar(m.confidence.sqrt(), phi);
        psi = psi.add(amplitude.scale(kernel));
    }

    psi
}

/// Evaluate |ψ(θ)|² — probability density from complex interference.
///
/// This is the complex-framework analog of evaluate_pattern().
/// When all phases are 0, this reduces to (Σₖ √cₖ·K(θ-θₖ))²,
/// which differs from the real framework's Σₖ cₖ·K(θ-θₖ) + cross terms
/// because the cross terms emerge naturally from |Σ|² rather than
/// being explicitly computed.
pub fn evaluate_complex_probability(field: &InterferenceField, theta: &Angle) -> f64 {
    evaluate_complex_amplitude(field, theta).norm_sq()
}

/// Evaluate only the imaginary component of ψ(θ).
///
/// Im(ψ) captures phase-dependent interference that the real framework
/// discards. Non-zero Im(ψ) indicates unresolved phase structure —
/// measurements whose phases don't fully align or oppose.
///
/// Physical interpretation:
/// - Im(ψ) ≈ 0: phases are aligned (constructive) or anti-aligned (destructive)
/// - Im(ψ) >> 0 or << 0: phases are partially rotated → unresolved structure
pub fn evaluate_complex_imaginary(field: &InterferenceField, theta: &Angle) -> f64 {
    evaluate_complex_amplitude(field, theta).im
}

/// Evaluate only the real component of ψ(θ).
pub fn evaluate_complex_real(field: &InterferenceField, theta: &Angle) -> f64 {
    evaluate_complex_amplitude(field, theta).re
}

// ─── Imaginary Probability Detection (虚概率检测) ──────────────────────
//
// Before collapse, detect whether the imaginary component of ψ(θ_peak)
// is significant relative to |ψ(θ_peak)|. If so, there's unresolved
// phase information that warrants additional measurements rather than
// premature collapse.
//
// Decision rule:
// - |Im(ψ)| / |ψ| < threshold → phases resolved → safe to collapse
// - |Im(ψ)| / |ψ| > threshold → unresolved phase structure → supplement

/// Detect significant imaginary component — 虚概率检测.
///
/// Checks if Im(ψ(θ_peak)) is significant relative to |ψ(θ_peak)|.
/// If the ratio exceeds the threshold, there's unresolved phase
/// information → additional measurements recommended before collapse.
///
/// Returns (im_magnitude, total_magnitude, should_supplement).
pub fn detect_imaginary_probability(
    field: &InterferenceField,
    peak: &Angle,
    threshold: f64,
) -> (f64, f64, bool) {
    let psi = evaluate_complex_amplitude(field, peak);
    let im_mag = psi.im.abs();
    let total_mag = psi.norm();
    let should_supplement = total_mag > f64::EPSILON && im_mag / total_mag > threshold;
    (im_mag, total_mag, should_supplement)
}

/// Scan all angles and find where imaginary probability is strongest.
///
/// Returns the angle with maximum |Im(ψ(θ))| and its value.
/// Useful for identifying where phase structure is most unresolved.
pub fn max_imaginary_angle(field: &InterferenceField, resolution: usize) -> (Angle, f64) {
    (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            let angle = Angle::from_degrees(deg);
            let im = evaluate_complex_imaginary(field, &angle).abs();
            (angle, im)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap_or((Angle::from_degrees(0.0), 0.0))
}

/// Find the complex probability peak (maximum |ψ(θ)|²).
pub fn find_complex_peak(field: &InterferenceField, resolution: usize) -> Angle {
    (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            let angle = Angle::from_degrees(deg);
            let val = evaluate_complex_probability(field, &angle);
            (angle, val)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(a, _)| a)
        .unwrap_or_else(|| Angle::from_degrees(0.0))
}

/// Complex contrast ratio: |ψ|²_max / (|ψ|²_median + ε).
pub fn complex_contrast_ratio(field: &InterferenceField, resolution: usize) -> f64 {
    let patterns: Vec<f64> = (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            evaluate_complex_probability(field, &Angle::from_degrees(deg))
        })
        .collect();

    let i_max = patterns.iter().cloned().fold(0.0f64, f64::max);
    let mut sorted = patterns.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i_median = sorted[sorted.len() / 2];
    let epsilon = 1e-8;

    i_max / (i_median + epsilon)
}

// ─── Pattern Topology Metrics ───────────────────────────────────────────

/// Compute pattern contrast ratio: CR = I_max / (I_median + ε).
///
/// Same semantics as density.rs contrast_ratio:
/// - CR ≈ 1 → flat (no structure)
/// - CR >> 1 → clear peak (strong structure)
pub fn contrast_ratio(field: &InterferenceField, resolution: usize) -> f64 {
    let patterns: Vec<f64> = (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            evaluate_pattern(field, &Angle::from_degrees(deg))
        })
        .collect();

    let i_max = patterns.iter().cloned().fold(0.0f64, f64::max);
    let mut sorted = patterns.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i_median = sorted[sorted.len() / 2];
    let epsilon = 1e-8;

    // Handle negative interference: if max is negative, field is all-destructive
    if i_max < 0.0 {
        return 0.0;
    }

    i_max / (i_median.abs() + epsilon)
}

/// Compute transit threshold: τ_transit(N) = τ_base · (1 + β / max(N-N_min, 1)).
///
/// Same as density.rs — conservative early, relaxed later.
pub fn transit_threshold(field: &InterferenceField) -> f64 {
    let n = field.measurements.len();
    let n_diff = (n as isize - field.n_min as isize).max(1) as f64;
    field.tau_base * (1.0 + field.beta / n_diff)
}

/// Check if transition condition is met: CR > τ_transit(N).
///
/// Switches from active search (Phase 2) to gradient descent (Phase 4).
pub fn should_transition(field: &InterferenceField, resolution: usize) -> bool {
    if field.measurements.len() < field.n_min {
        return false;
    }
    let cr = contrast_ratio(field, resolution);
    let tau = transit_threshold(field);
    cr > tau
}

// ─── Gradient Descent ───────────────────────────────────────────────────

/// Numerical gradient of I(θ) at the given angle.
///
/// dI/dθ ≈ (I(θ+h) - I(θ-h)) / (2h)
///
/// This is the **interference gradient** (strategy B): the direction
/// where the interference pattern changes most rapidly = highest
/// information gain region.
pub fn pattern_gradient(field: &InterferenceField, angle: &Angle) -> f64 {
    let h = 1e-3;
    let plus = evaluate_pattern(field, &Angle::from_degrees(angle.degrees + h));
    let minus = evaluate_pattern(field, &Angle::from_degrees(angle.degrees - h));
    (plus - minus) / (2.0 * h)
}

/// Gradient magnitude |dI/dθ| at the given angle.
pub fn pattern_gradient_magnitude(field: &InterferenceField, angle: &Angle) -> f64 {
    pattern_gradient(field, angle).abs()
}

/// One gradient descent step on the interference pattern.
///
/// θ_{t+1} = θ_t + α · dI/dθ|_{θ_t}
///
/// Ascends toward the pattern peak (maximum interference structure).
pub fn gradient_descent_step(field: &InterferenceField, current: &Angle, step_size: f64) -> Angle {
    let grad = pattern_gradient(field, current);
    Angle::from_degrees(current.degrees + step_size * grad)
}

// ─── Strategy B: Interference Gradient Measurement Selection ────────────

/// Find the angle with maximum interference gradient magnitude.
///
/// This is strategy B from the architecture doc: select the direction
/// where the interference pattern changes most剧烈. High gradient =
/// rich superposition structure = maximum information gain.
pub fn max_gradient_angle(field: &InterferenceField, resolution: usize) -> Angle {
    (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            let angle = Angle::from_degrees(deg);
            let mag = pattern_gradient_magnitude(field, &angle);
            (angle, mag)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(a, _)| a)
        .unwrap_or_else(|| Angle::from_degrees(0.0))
}

/// Find the pattern peak (maximum I(θ)).
pub fn find_pattern_peak(field: &InterferenceField, resolution: usize) -> Angle {
    (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            let angle = Angle::from_degrees(deg);
            let val = evaluate_pattern(field, &angle);
            (angle, val)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(a, _)| a)
        .unwrap_or_else(|| Angle::from_degrees(0.0))
}

// ─── Final Collapse ─────────────────────────────────────────────────────

/// Pattern-weighted random sampling within the FWHM of the peak.
///
/// NOT argmax — preserves randomness. P(θ) ∝ I(θ) within the peak region.
/// The "random but reliable" property.
///
/// Uses a deterministic seed for reproducibility (same as density.rs).
pub fn pattern_weighted_sample(
    field: &InterferenceField,
    peak: &Angle,
    fwhm: f64,
) -> (Angle, CollapseMethod) {
    let range = fwhm / 2.0;
    let samples: Vec<(Angle, f64)> = (0..100)
        .map(|i| {
            let offset = (i as f64 / 99.0 - 0.5) * 2.0 * range;
            let angle = Angle::from_degrees(peak.degrees + offset);
            let weight = evaluate_pattern(field, &angle).max(0.0); // clamp negative
            (angle, weight)
        })
        .collect();

    let total_weight: f64 = samples.iter().map(|(_, w)| w).sum();
    if total_weight < f64::EPSILON {
        return (*peak, CollapseMethod::WeightedRandom);
    }

    // Deterministic seed for reproducibility
    let rng_seed = 42u64;
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

/// Compute FWHM (Full Width at Half Maximum) around a peak in the pattern.
///
/// Scans outward from the peak until I(θ) drops below half-max.
/// Returns angular width in degrees, clamped to [10°, 180°].
pub fn compute_fwhm(
    field: &InterferenceField,
    peak: &Angle,
    min_fwhm: f64,
    default_fwhm: f64,
) -> f64 {
    let peak_val = evaluate_pattern(field, peak);
    let half_max = peak_val / 2.0;

    if half_max < f64::EPSILON {
        return default_fwhm;
    }

    let mut left_offset: Option<f64> = None;
    let mut right_offset: Option<f64> = None;

    for i in 1..=180 {
        if left_offset.is_none() {
            let left_angle = Angle::from_degrees(peak.degrees - i as f64);
            let left_val = evaluate_pattern(field, &left_angle);
            if left_val < half_max {
                left_offset = Some(i as f64);
            }
        }

        if right_offset.is_none() {
            let right_angle = Angle::from_degrees(peak.degrees + i as f64);
            let right_val = evaluate_pattern(field, &right_angle);
            if right_val < half_max {
                right_offset = Some(i as f64);
            }
        }

        if left_offset.is_some() && right_offset.is_some() {
            break;
        }
    }

    let left = left_offset.unwrap_or(90.0);
    let right = right_offset.unwrap_or(90.0);
    (left + right).max(min_fwhm).min(180.0)
}

// ─── Destructive Interference Detection (for Resolution Layer) ──────────

/// Find angles where destructive interference dominates (I(θ) < 0).
///
/// These are "contradiction points" — locations where measurement bases
/// disagree. The resolution layer (Phase 3) should deep-dive here.
///
/// Returns a list of (angle, negativity) pairs, sorted by most negative first.
pub fn find_destructive_points(field: &InterferenceField, resolution: usize) -> Vec<(Angle, f64)> {
    let mut points: Vec<(Angle, f64)> = (0..resolution)
        .map(|i| {
            let deg = i as f64 * 360.0 / resolution as f64;
            let angle = Angle::from_degrees(deg);
            let val = evaluate_pattern(field, &angle);
            (angle, val)
        })
        .filter(|(_, v)| *v < 0.0)
        .collect();

    points.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    points
}

/// Compute the angular resolution direction: the shift from old peak to new peak.
///
/// After supplementary measurements resolve destructive interference points,
/// the pattern peak may shift. This shift direction IS the collapse direction
/// — "矛盾消解方向=坍缩方向".
///
/// Returns (new_peak, signed_shift_degrees) where positive shift = clockwise
/// (increasing degrees), negative = counterclockwise.
pub fn resolution_direction(
    field: &InterferenceField,
    old_peak: &Angle,
    resolution: usize,
) -> (Angle, f64) {
    let new_peak = find_pattern_peak(field, resolution);
    let shift = circular_diff(new_peak.degrees, old_peak.degrees);
    // Sign the shift: determine direction
    let raw = (new_peak.degrees - old_peak.degrees).rem_euclid(360.0);
    let signed_shift = if raw <= 180.0 { shift } else { -shift };
    (new_peak, signed_shift)
}

// ─── Slerp Collapse (P1: Primary Contradiction) ────────────────────────
//
// Replaces additive weighted sampling with norm-preserving rotation.
// Inspired by Spherical Steering (arXiv 2602.08169): collapse is a
// rotation along the geodesic, NOT an additive blend.

/// Slerp (Spherical Linear Interpolation) collapse.
///
/// Replaces `pattern_weighted_sample` with norm-preserving rotation
/// along the geodesic between the peak and the secondary interference
/// direction within the FWHM.
///
/// Key difference from weighted sampling:
/// - Weighted sampling: P(theta) proportional to I(theta) -- additive blend
/// - Slerp: theta_collapse = Slerp(theta_peak, theta_secondary, t) -- rotational
///   interpolation that preserves signal integrity.
///
/// The interpolation parameter t is the interference strength ratio:
///   t = I(theta_secondary) / (I(theta_peak) + I(theta_secondary))
pub fn slerp_collapse(
    field: &InterferenceField,
    peak: &Angle,
    fwhm: f64,
) -> (Angle, CollapseMethod) {
    let range = fwhm / 2.0;

    // Find secondary peak within FWHM range (excluding immediate peak vicinity)
    let secondary = find_secondary_peak_in_range(field, peak, range);

    match secondary {
        Some((sec_angle, sec_intensity)) => {
            let peak_intensity = evaluate_pattern(field, peak).max(0.0);

            if peak_intensity < f64::EPSILON {
                return (*peak, CollapseMethod::SlerpRotation);
            }

            // Slerp interpolation parameter: ratio of secondary to total
            let t = sec_intensity / (peak_intensity + sec_intensity);

            // Apply vMF confidence gating (P2) to modulate interpolation strength
            let kappa = compute_vmf_kappa(field, peak);
            let kappa_0 = 1.0; // Calibration parameter
            let gate = vmf_gate(kappa, kappa_0);

            // Effective interpolation: gated by vMF confidence
            let t_effective = t * gate;

            // Circular Slerp: interpolate along shortest arc
            let collapsed = circular_slerp(peak, &sec_angle, t_effective);

            (collapsed, CollapseMethod::SlerpRotation)
        }
        None => {
            // No secondary peak found -- collapse to peak
            (*peak, CollapseMethod::SlerpRotation)
        }
    }
}

/// Circular Slerp: interpolate along the shortest arc between two angles.
///
/// On a 1D circle, Slerp degenerates to linear interpolation along the
/// shortest arc. The key property preserved is that the interpolation
/// follows the geodesic (shortest path), not an additive blend.
fn circular_slerp(a: &Angle, b: &Angle, t: f64) -> Angle {
    // Find shortest arc direction (signed)
    let diff = (b.degrees - a.degrees).rem_euclid(360.0);
    let shortest = if diff <= 180.0 { diff } else { diff - 360.0 };

    Angle::from_degrees(a.degrees + t * shortest)
}

/// Find the second strongest interference direction within a range.
///
/// Excludes the immediate vicinity of the peak (+/- 5 degrees) to find a
/// genuinely different direction. Returns (angle, intensity) or None.
fn find_secondary_peak_in_range(
    field: &InterferenceField,
    peak: &Angle,
    range: f64,
) -> Option<(Angle, f64)> {
    let samples: usize = 100;
    let mut best: Option<(Angle, f64)> = None;

    for i in 0..samples {
        let offset = (i as f64 / (samples - 1) as f64 - 0.5) * 2.0 * range;
        let angle = Angle::from_degrees(peak.degrees + offset);

        // Exclude immediate peak vicinity (+/- 5 degrees)
        if offset.abs() < 5.0 {
            continue;
        }

        let intensity = evaluate_pattern(field, &angle).max(0.0);

        match best {
            None => best = Some((angle, intensity)),
            Some((_, best_val)) if intensity > best_val => {
                best = Some((angle, intensity));
            }
            _ => {}
        }
    }

    best
}

// ─── vMF Confidence Gating (P2: Secondary Contradiction) ───────────────
//
// Replaces fixed tau=0.5 consistency threshold with adaptive gating
// based on von Mises-Fisher concentration parameter kappa.
// Inspired by Spherical Steering's confidence gate.

/// Compute von Mises-Fisher concentration parameter kappa.
///
/// kappa measures how concentrated the measurement results are around
/// their mean direction. High kappa = high consistency = strong collapse
/// signal. Low kappa = scattered measurements = weak signal.
pub fn compute_vmf_kappa(field: &InterferenceField, peak: &Angle) -> f64 {
    let n = field.measurements.len();
    if n == 0 {
        return 0.0;
    }

    let dim = field.measurements[0].result_embedding.len();
    if dim == 0 {
        return 0.0;
    }

    // Compute mean resultant length R_bar from result embeddings
    let mut sum_vec = vec![0.0; dim];
    for m in &field.measurements {
        let norm: f64 = m
            .result_embedding
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
            .max(f64::EPSILON);
        for (i, &v) in m.result_embedding.iter().enumerate() {
            sum_vec[i] += v / norm;
        }
    }

    // R_bar = |sum of unit vectors| / n
    let sum_norm: f64 = sum_vec.iter().map(|v| v * v).sum::<f64>().sqrt();
    let r_bar = sum_norm / n as f64;

    // Local pattern standard deviation around peak (+/- 90 degrees in 5 degree steps)
    let local_values: Vec<f64> = (0..37)
        .map(|i| {
            let offset = (i as f64 - 18.0) * 5.0;
            evaluate_pattern(field, &Angle::from_degrees(peak.degrees + offset))
        })
        .collect();

    let mean = local_values.iter().sum::<f64>() / local_values.len() as f64;
    let variance =
        local_values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / local_values.len() as f64;
    let sigma = variance.sqrt().max(f64::EPSILON);

    // kappa = R_bar / (n * sigma)
    let kappa = r_bar / (n as f64 * sigma);

    kappa.max(0.0)
}

/// vMF-based confidence gate.
///
/// Maps the concentration parameter kappa to a gate value in [0, 1].
/// High kappa -> gate approx 1 (strong collapse, high confidence).
/// Low kappa -> gate approx 0 (weak collapse, low confidence).
pub fn vmf_gate(kappa: f64, kappa_0: f64) -> f64 {
    1.0 - (-kappa / kappa_0.max(f64::EPSILON)).exp()
}

// ─── Contrastive Prototype Direction (P3: Expanding Scope) ─────────────
//
// Inspired by CAA (Contrastive Activation Addition): extract contrastive
// direction from the measurement pair with strongest interference.

/// Find the contrastive prototype direction from measurement pairs.
///
/// Inspired by CAA (arXiv 2308.10248): find the pair of measurements
/// with the strongest interference (constructive or destructive), and
/// extract their result-embedding difference as a "contrastive direction".
///
/// Returns (direction_vector, interference_weight) or None if < 2 measurements.
pub fn find_contrast_prototype(field: &InterferenceField) -> Option<(Embedding, f64)> {
    let n = field.measurements.len();
    if n < 2 {
        return None;
    }

    let mut best_pair: Option<(usize, usize, f64)> = None;

    for i in 0..n {
        for j in (i + 1)..n {
            let mi = &field.measurements[i];
            let mj = &field.measurements[j];

            let alpha_ij = (mi.confidence * mj.confidence).sqrt();
            let cos_phase = cosine_sim(&mi.basis_embedding, &mj.basis_embedding);
            let result_sim = cosine_sim(&mi.result_embedding, &mj.result_embedding);
            let consistency_gate = if result_sim > field.consistency_threshold {
                1.0
            } else {
                -1.0
            };

            // Cross-term magnitude = interference strength
            let cross_magnitude = (2.0 * alpha_ij * cos_phase * consistency_gate).abs();

            match best_pair {
                None => best_pair = Some((i, j, cross_magnitude)),
                Some((_, _, best_val)) if cross_magnitude > best_val => {
                    best_pair = Some((i, j, cross_magnitude));
                }
                _ => {}
            }
        }
    }

    let (i, j, weight) = best_pair?;

    // Compute contrastive direction: normalize(e_i - e_j)
    let dim = field.measurements[i].result_embedding.len();
    if dim == 0 {
        return None;
    }

    let mut diff: Vec<f64> = (0..dim)
        .map(|k| {
            field.measurements[i].result_embedding[k] - field.measurements[j].result_embedding[k]
        })
        .collect();

    let norm: f64 = diff
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
        .max(f64::EPSILON);
    for v in &mut diff {
        *v /= norm;
    }

    Some((diff, weight))
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a simple embedding vector.
    fn emb(vals: &[f64]) -> Embedding {
        let norm: f64 = vals
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
            .max(f64::EPSILON);
        vals.iter().map(|v| v / norm).collect()
    }

    #[test]
    fn cosine_sim_identical_vectors() {
        let v = emb(&[1.0, 2.0, 3.0]);
        assert!((cosine_sim(&v, &v) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_sim_orthogonal_vectors() {
        let a = emb(&[1.0, 0.0]);
        let b = emb(&[0.0, 1.0]);
        assert!(cosine_sim(&a, &b).abs() < 1e-9);
    }

    #[test]
    fn cosine_sim_opposite_vectors() {
        let a = emb(&[1.0, 0.0]);
        let b = emb(&[-1.0, 0.0]);
        assert!((cosine_sim(&a, &b) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn circular_diff_handles_wraparound() {
        assert!((circular_diff(350.0, 10.0) - 20.0).abs() < 1e-9);
        assert!((circular_diff(10.0, 350.0) - 20.0).abs() < 1e-9);
        assert!((circular_diff(180.0, 180.0)).abs() < 1e-9);
    }

    #[test]
    fn circular_midpoint_short_arc() {
        let mid = circular_midpoint(10.0, 30.0);
        assert!((mid - 20.0).abs() < 1e-9);
    }

    #[test]
    fn circular_midpoint_wraps_around_zero() {
        let mid = circular_midpoint(350.0, 10.0);
        assert!((mid - 0.0).abs() < 1e-9 || (mid - 360.0).abs() < 1e-9);
    }

    #[test]
    fn gaussian_kernel_peaks_at_zero() {
        let k0 = gaussian_kernel(0.0, 10.0);
        let k1 = gaussian_kernel(5.0, 10.0);
        assert!(k0 > k1);
        assert!((k0 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn empty_field_pattern_is_zero() {
        let field = InterferenceField::new();
        let val = evaluate_pattern(&field, &Angle::from_degrees(90.0));
        assert!(val.abs() < f64::EPSILON);
    }

    #[test]
    fn single_measurement_matches_independent_term() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(180.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );

        // With one measurement, cross term is zero, so I(θ) = c₀·K(θ-θ₀)
        let val_at_peak = evaluate_pattern(&field, &Angle::from_degrees(180.0));
        let val_away = evaluate_pattern(&field, &Angle::from_degrees(0.0));
        assert!(val_at_peak > val_away);
        assert!((val_at_peak - 1.0).abs() < 1e-9); // K(0) = 1, c = 1
    }

    #[test]
    fn two_similar_bases_constructive_interference() {
        let mut field = InterferenceField::new();
        // Two measurements with similar bases and similar results
        // → constructive interference (cross term positive)
        field.add_measurement(
            Angle::from_degrees(90.0),
            0.8,
            emb(&[1.0, 0.1]), // similar basis
            emb(&[1.0, 0.1]), // similar result
        );
        field.add_measurement(
            Angle::from_degrees(100.0),
            0.8,
            emb(&[1.0, 0.1]), // same basis direction
            emb(&[1.0, 0.1]), // same result direction
        );

        let cross = evaluate_cross_term(&field, &Angle::from_degrees(95.0));
        // cos_sim of identical bases = 1.0, result_sim = 1.0 > τ → gate = +1
        // cross should be positive (constructive)
        assert!(
            cross > 0.0,
            "similar bases should give constructive interference, got {cross}"
        );
    }

    #[test]
    fn two_dissimilar_results_destructive_interference() {
        let mut field = InterferenceField::new();
        field.consistency_threshold = 0.3;

        // Similar bases but contradictory results
        field.add_measurement(
            Angle::from_degrees(90.0),
            0.8,
            emb(&[1.0, 0.1]), // similar basis
            emb(&[1.0, 0.0]), // result A
        );
        field.add_measurement(
            Angle::from_degrees(100.0),
            0.8,
            emb(&[1.0, 0.1]),  // similar basis
            emb(&[-1.0, 0.0]), // result B = opposite of A
        );

        let cross = evaluate_cross_term(&field, &Angle::from_degrees(95.0));
        // cos_sim(basis) ≈ 1.0 (positive), but result_sim < 0 < τ → gate = -1
        // cross should be negative (destructive)
        assert!(
            cross < 0.0,
            "similar bases + contradictory results should give destructive interference, got {cross}"
        );
    }

    #[test]
    fn orthogonal_bases_no_interference() {
        let mut field = InterferenceField::new();
        // Orthogonal bases → cos_sim = 0 → cross term = 0
        field.add_measurement(
            Angle::from_degrees(90.0),
            0.8,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        field.add_measurement(
            Angle::from_degrees(100.0),
            0.8,
            emb(&[0.0, 1.0]), // orthogonal basis
            emb(&[1.0, 0.0]),
        );

        let cross = evaluate_cross_term(&field, &Angle::from_degrees(95.0));
        assert!(
            cross.abs() < 1e-9,
            "orthogonal bases should have zero cross term, got {cross}"
        );
    }

    #[test]
    fn contrast_ratio_empty_field() {
        let field = InterferenceField::new();
        let cr = contrast_ratio(&field, 36);
        assert!(cr < 1.0);
    }

    #[test]
    fn contrast_ratio_with_structure() {
        let mut field = InterferenceField::new();
        for deg in [90.0, 95.0, 100.0] {
            field.add_measurement(
                Angle::from_degrees(deg),
                0.9,
                emb(&[1.0, 0.1]),
                emb(&[1.0, 0.1]),
            );
        }
        let cr = contrast_ratio(&field, 72);
        // With a cluster of similar measurements, CR should be > 1
        assert!(
            cr > 1.0,
            "clustered measurements should give CR > 1, got {cr}"
        );
    }

    #[test]
    fn transit_threshold_decreases_with_n() {
        let field = InterferenceField::new();
        // N=0: n_diff = max(-5, 1) = 1 → τ = 2*(1+2) = 6.0
        let t = transit_threshold(&field);
        assert!((t - 6.0).abs() < 0.1);
    }

    #[test]
    fn pattern_gradient_is_non_negative_magnitude() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        let mag = pattern_gradient_magnitude(&field, &Angle::from_degrees(90.0));
        assert!(mag >= 0.0);
    }

    #[test]
    fn max_gradient_angle_returns_valid_angle() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        let angle = max_gradient_angle(&field, 36);
        assert!(angle.degrees >= 0.0 && angle.degrees < 360.0);
    }

    #[test]
    fn find_destructive_points_detects_negatives() {
        let mut field = InterferenceField::new();
        field.consistency_threshold = 0.3;
        field.add_measurement(
            Angle::from_degrees(90.0),
            0.8,
            emb(&[1.0, 0.1]),
            emb(&[1.0, 0.0]),
        );
        field.add_measurement(
            Angle::from_degrees(95.0),
            0.8,
            emb(&[1.0, 0.1]),
            emb(&[-1.0, 0.0]), // contradictory
        );

        let destructive = find_destructive_points(&field, 72);
        // Should find at least some destructive points
        assert!(
            !destructive.is_empty(),
            "contradictory measurements should produce destructive points"
        );
    }

    #[test]
    fn fwhm_returns_default_for_empty_field() {
        let field = InterferenceField::new();
        let fwhm = compute_fwhm(&field, &Angle::from_degrees(0.0), 10.0, 30.0);
        assert!((fwhm - 30.0).abs() < 1e-9);
    }

    #[test]
    fn fwhm_with_cluster_is_in_range() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(180.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        let fwhm = compute_fwhm(&field, &Angle::from_degrees(180.0), 10.0, 30.0);
        assert!(fwhm >= 10.0 && fwhm <= 180.0);
    }

    #[test]
    fn pattern_weighted_sample_returns_peak_for_sharp_field() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(180.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        let (angle, method) = pattern_weighted_sample(&field, &Angle::from_degrees(180.0), 20.0);
        assert!(angle.degrees >= 170.0 && angle.degrees <= 190.0);
        assert!(matches!(method, CollapseMethod::WeightedRandom));
    }

    // ── Slerp Collapse Tests (P1) ──────────────────────────────────

    #[test]
    fn circular_slerp_short_arc() {
        let a = Angle::from_degrees(10.0);
        let b = Angle::from_degrees(30.0);
        let mid = circular_slerp(&a, &b, 0.5);
        assert!((mid.degrees - 20.0).abs() < 1e-6);
    }

    #[test]
    fn circular_slerp_wraps_around_zero() {
        let a = Angle::from_degrees(350.0);
        let b = Angle::from_degrees(10.0);
        let mid = circular_slerp(&a, &b, 0.5);
        // Shortest arc crosses 0°: 350 → 0 → 10
        assert!((mid.degrees - 0.0).abs() < 1e-6 || (mid.degrees - 360.0).abs() < 1e-6);
    }

    #[test]
    fn circular_slerp_endpoints() {
        let a = Angle::from_degrees(90.0);
        let b = Angle::from_degrees(180.0);
        let at_0 = circular_slerp(&a, &b, 0.0);
        let at_1 = circular_slerp(&a, &b, 1.0);
        assert!((at_0.degrees - 90.0).abs() < 1e-6);
        assert!((at_1.degrees - 180.0).abs() < 1e-6);
    }

    #[test]
    fn find_secondary_peak_excludes_immediate_vicinity() {
        let mut field = InterferenceField::new();
        // Peak at 180°, secondary at 190°
        field.add_measurement(
            Angle::from_degrees(180.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        field.add_measurement(
            Angle::from_degrees(190.0),
            0.5,
            emb(&[1.0, 0.1]),
            emb(&[1.0, 0.1]),
        );

        let secondary = find_secondary_peak_in_range(&field, &Angle::from_degrees(180.0), 30.0);
        assert!(secondary.is_some());
        let (sec_angle, _) = secondary.unwrap();
        // Should NOT be within 5° of 180°
        let diff = (sec_angle.degrees - 180.0).abs();
        let circ_diff = diff.min(360.0 - diff);
        assert!(
            circ_diff >= 5.0,
            "secondary peak should be >5° from peak, got {}°",
            circ_diff
        );
    }

    #[test]
    fn slerp_collapse_returns_slerp_method() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(180.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        field.add_measurement(
            Angle::from_degrees(200.0),
            0.6,
            emb(&[1.0, 0.1]),
            emb(&[1.0, 0.1]),
        );

        let (angle, method) = slerp_collapse(&field, &Angle::from_degrees(180.0), 40.0);
        assert!(matches!(method, CollapseMethod::SlerpRotation));
        // Should be between 180° and 200° (within FWHM)
        assert!(angle.degrees >= 175.0 && angle.degrees <= 205.0);
    }

    #[test]
    fn slerp_collapse_single_measurement_returns_peak() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );

        let (angle, method) = slerp_collapse(&field, &Angle::from_degrees(90.0), 30.0);
        assert!((angle.degrees - 90.0).abs() < 1e-6);
        assert!(matches!(method, CollapseMethod::SlerpRotation));
    }

    // ── vMF Gating Tests (P2) ──────────────────────────────────────

    #[test]
    fn vmf_gate_high_kappa_approaches_one() {
        let gate = vmf_gate(100.0, 1.0);
        assert!(gate > 0.99, "high κ should give gate ≈ 1, got {}", gate);
    }

    #[test]
    fn vmf_gate_low_kappa_approaches_zero() {
        let gate = vmf_gate(0.001, 1.0);
        assert!(gate < 0.01, "low κ should give gate ≈ 0, got {}", gate);
    }

    #[test]
    fn vmf_gate_zero_kappa_is_zero() {
        let gate = vmf_gate(0.0, 1.0);
        assert!(gate.abs() < 1e-9);
    }

    #[test]
    fn compute_vmf_kappa_empty_field_is_zero() {
        let field = InterferenceField::new();
        let kappa = compute_vmf_kappa(&field, &Angle::from_degrees(0.0));
        assert!(kappa.abs() < 1e-9);
    }

    #[test]
    fn compute_vmf_kappa_consistent_measurements_is_positive() {
        let mut field = InterferenceField::new();
        // All measurements have similar result embeddings → high R̄
        for deg in [90.0, 95.0, 100.0] {
            field.add_measurement(
                Angle::from_degrees(deg),
                0.9,
                emb(&[1.0, 0.1]),
                emb(&[1.0, 0.1]), // same result direction
            );
        }
        let kappa = compute_vmf_kappa(&field, &Angle::from_degrees(95.0));
        assert!(
            kappa > 0.0,
            "consistent measurements should give κ > 0, got {}",
            kappa
        );
    }

    // ── Contrastive Prototype Tests (P3) ───────────────────────────

    #[test]
    fn contrast_prototype_empty_field_returns_none() {
        let field = InterferenceField::new();
        assert!(find_contrast_prototype(&field).is_none());
    }

    #[test]
    fn contrast_prototype_single_measurement_returns_none() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        assert!(find_contrast_prototype(&field).is_none());
    }

    #[test]
    fn contrast_prototype_two_measurements_returns_direction() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(90.0),
            0.8,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]), // result A
        );
        field.add_measurement(
            Angle::from_degrees(100.0),
            0.8,
            emb(&[1.0, 0.1]),
            emb(&[-1.0, 0.0]), // result B = opposite of A
        );

        let prototype = find_contrast_prototype(&field);
        assert!(prototype.is_some());
        let (direction, weight) = prototype.unwrap();
        assert!(!direction.is_empty());
        assert!(weight > 0.0);
    }

    #[test]
    fn contrast_prototype_direction_is_normalized() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(0.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[0.0, 1.0]),
            emb(&[0.0, 1.0]),
        );

        let (direction, _) = find_contrast_prototype(&field).unwrap();
        let norm: f64 = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-6,
            "direction should be normalized, norm = {}",
            norm
        );
    }

    // ── Complex Framework Tests ────────────────────────────────────

    #[test]
    fn complex_type_basic_operations() {
        let z1 = Complex::new(3.0, 4.0);
        assert!((z1.norm() - 5.0).abs() < 1e-9);
        assert!((z1.norm_sq() - 25.0).abs() < 1e-9);

        let z2 = Complex::from_polar(1.0, std::f64::consts::FRAC_PI_2);
        assert!(z2.re.abs() < 1e-9);
        assert!((z2.im - 1.0).abs() < 1e-9);

        let z3 = Complex::new(1.0, 2.0);
        let z4 = Complex::new(3.0, 4.0);
        let sum = z3.add(z4);
        assert!((sum.re - 4.0).abs() < 1e-9);
        assert!((sum.im - 6.0).abs() < 1e-9);

        let prod = z3.mul(z4);
        // (1+2i)(3+4i) = 3+4i+6i+8i² = 3-8+10i = -5+10i
        assert!((prod.re - (-5.0)).abs() < 1e-9);
        assert!((prod.im - 10.0).abs() < 1e-9);

        let conj = z3.conjugate();
        assert!((conj.re - 1.0).abs() < 1e-9);
        assert!((conj.im - (-2.0)).abs() < 1e-9);
    }

    #[test]
    fn complex_amplitude_empty_field_is_zero() {
        let field = InterferenceField::new();
        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(90.0));
        assert!(psi.norm() < f64::EPSILON);
    }

    #[test]
    fn complex_amplitude_single_measurement_no_phase() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(180.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );

        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(180.0));
        // No phase → α = √1 · e^{i·0} = 1 (real)
        // K(0, h) = 1, so ψ = 1 + 0i
        assert!((psi.re - 1.0).abs() < 1e-9);
        assert!(psi.im.abs() < 1e-9);
    }

    #[test]
    fn complex_amplitude_with_phase_produces_imaginary() {
        let mut field = InterferenceField::new();
        // Single measurement with phase = π/2
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(std::f64::consts::FRAC_PI_2),
        );

        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(90.0));
        // α = √1 · e^{iπ/2} = cos(π/2) + i·sin(π/2) = 0 + i
        // K(0) = 1, so ψ = 0 + 1i
        assert!(psi.re.abs() < 1e-9);
        assert!((psi.im - 1.0).abs() < 1e-9);
    }

    #[test]
    fn complex_probability_equals_norm_squared() {
        let mut field = InterferenceField::new();
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            0.8,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(0.5),
        );

        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(90.0));
        let prob = evaluate_complex_probability(&field, &Angle::from_degrees(90.0));
        assert!((prob - psi.norm_sq()).abs() < 1e-12);
    }

    #[test]
    fn two_opposite_phases_produce_destructive_interference() {
        let mut field = InterferenceField::new();
        // Two measurements at same angle, opposite phases
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(0.0), // phase = 0
        );
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(std::f64::consts::PI), // phase = π (opposite)
        );

        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(90.0));
        // α₁ = 1·e^{i·0} = 1, α₂ = 1·e^{iπ} = -1
        // ψ = 1·K(0) + (-1)·K(0) = 0
        assert!(
            psi.norm() < 1e-9,
            "opposite phases should cancel, got |ψ| = {}",
            psi.norm()
        );
    }

    #[test]
    fn two_aligned_phases_produce_constructive_interference() {
        let mut field = InterferenceField::new();
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(0.0),
        );
        field.add_measurement_with_phase(
            Angle::from_degrees(95.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(0.0),
        );

        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(92.5));
        // Both phases = 0, so ψ is real and positive
        assert!(
            psi.re > 0.0,
            "aligned phases should give positive real ψ, got {}",
            psi.re
        );
        assert!(
            psi.im.abs() < 1e-9,
            "zero phases should give zero imaginary, got {}",
            psi.im
        );
    }

    #[test]
    fn detect_imaginary_no_phase_returns_zero_ratio() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );

        let (im_mag, total_mag, should_supp) =
            detect_imaginary_probability(&field, &Angle::from_degrees(90.0), 0.1);
        // No phase → ψ is real → Im(ψ) = 0
        assert!(im_mag < 1e-9);
        assert!(!should_supp);
        assert!(total_mag > 0.0);
    }

    #[test]
    fn detect_imaginary_with_mixed_phases_triggers() {
        let mut field = InterferenceField::new();
        // Two measurements with different phases at same angle
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(0.0),
        );
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(std::f64::consts::FRAC_PI_2), // 90° phase
        );

        let (im_mag, total_mag, should_supp) =
            detect_imaginary_probability(&field, &Angle::from_degrees(90.0), 0.1);
        // ψ = 1 + i → Im/|ψ| = 1/√2 ≈ 0.707 > 0.1
        assert!(
            im_mag > 0.5,
            "mixed phases should produce significant Im, got {}",
            im_mag
        );
        assert!(should_supp, "should recommend supplementation");
    }

    #[test]
    fn complex_peak_finds_maximum_probability() {
        let mut field = InterferenceField::new();
        for deg in [90.0, 95.0, 100.0] {
            field.add_measurement_with_phase(
                Angle::from_degrees(deg),
                0.9,
                emb(&[1.0, 0.1]),
                emb(&[1.0, 0.1]),
                Some(0.0), // aligned phases
            );
        }

        let peak = find_complex_peak(&field, 72);
        let peak_val = evaluate_complex_probability(&field, &peak);
        let off_val = evaluate_complex_probability(&field, &Angle::from_degrees(270.0));
        assert!(peak_val > off_val, "peak should have higher probability");
        assert!(
            peak.degrees >= 80.0 && peak.degrees <= 110.0,
            "peak should be near 90-100°, got {}",
            peak.degrees
        );
    }

    #[test]
    fn complex_contrast_ratio_aligned_phases_high() {
        let mut field = InterferenceField::new();
        for deg in [90.0, 95.0, 100.0] {
            field.add_measurement_with_phase(
                Angle::from_degrees(deg),
                0.9,
                emb(&[1.0, 0.1]),
                emb(&[1.0, 0.1]),
                Some(0.0),
            );
        }
        let cr = complex_contrast_ratio(&field, 72);
        assert!(cr > 1.0, "aligned cluster should give CR > 1, got {}", cr);
    }

    #[test]
    fn max_imaginary_angle_finds_phase_conflict() {
        let mut field = InterferenceField::new();
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(0.0),
        );
        field.add_measurement_with_phase(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
            Some(std::f64::consts::FRAC_PI_2),
        );

        let (angle, im_val) = max_imaginary_angle(&field, 72);
        assert!(im_val > 0.5, "should find significant imaginary component");
        // Maximum imaginary should be near 90° where both measurements are
        assert!(
            angle.degrees >= 80.0 && angle.degrees <= 100.0,
            "max imaginary should be near measurements, got {}",
            angle.degrees
        );
    }

    #[test]
    fn set_phases_switches_to_complex_mode() {
        let mut field = InterferenceField::new();
        field.add_measurement(
            Angle::from_degrees(0.0),
            1.0,
            emb(&[1.0, 0.0]),
            emb(&[1.0, 0.0]),
        );
        field.add_measurement(
            Angle::from_degrees(90.0),
            1.0,
            emb(&[0.0, 1.0]),
            emb(&[0.0, 1.0]),
        );

        assert!(!field.has_phases(), "should start in real mode");

        field.set_phases(&[0.0, std::f64::consts::FRAC_PI_2]);
        assert!(
            field.has_phases(),
            "should be in complex mode after set_phases"
        );

        // Now evaluate_complex_amplitude should use the phases
        let psi = evaluate_complex_amplitude(&field, &Angle::from_degrees(0.0));
        // Measurement 0: α = 1·e^{i·0}·K(0) = 1
        // Measurement 1: α = 1·e^{iπ/2}·K(90°) ≈ 0 (far away)
        // ψ ≈ 1 + 0i
        assert!(
            (psi.re - 1.0).abs() < 0.1,
            "should be mostly real at measurement 0 angle"
        );
    }
}
