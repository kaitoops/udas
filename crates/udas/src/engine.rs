//! UDAS Engine — orchestrates the full search→collapse pipeline.
//!
//! ## Pipeline Phases (Interference Model)
//!
//! 1. **Cold Start** — 3 restorations at fixed angles (0°, 120°, 240°) to
//!    establish the MDS coordinate system. Three non-collinear points
//!    uniquely define a 2D frame.
//! 2. **Active Search** — interference gradient-guided exploration (strategy B).
//!    Each new restoration adds a measurement to the interference field.
//!    When the contrast ratio exceeds the transit threshold, transition
//!    to Phase 4.
//! 4. **Gradient Descent** — numerical gradient ascent on I(θ) with
//!    adaptive step size (sigmoid-modulated). Converges to the pattern peak.
//! 5. **Final Collapse** — pattern-weighted random sampling within the FWHM
//!    of the peak. NOT argmax — preserves the "random but reliable" property.
//!
//! ## Interference Model (2026-07-25)
//!
//! Replaced additive KDE with quantum-inspired interference:
//! - Cross terms between measurement bases produce new information
//! - Constructive interference = consensus, destructive = contradiction
//! - Measurement selection driven by interference gradient (strategy B)
//!
//! ## Design Principles
//!
//! - The engine owns ALL state for a single search session.
//! - Each phase is independent and testable.
//! - Precision control (gap_1–4) is integrated but non-blocking.
//! - No hardcoded angles beyond cold-start initialisation.

use crate::breaker::{BreakerEvent, CircuitBreaker, TripStatus};
use crate::geometry;
use crate::interference::{self, InterferenceField};
use crate::memory::UdasMemory;
use crate::precision_control::{self, PrecisionController};
use crate::restoration::{self, LlmRestorer};
use crate::types::{Angle, CollapseMethod, Embedding, MeasurementInput, UdasOutput};
use serde::Serialize;
use udas_embedding::Embedder;

/// Pattern field resolution for CR computation (36 = every 10°).
const PATTERN_RESOLUTION: usize = 36;

/// Cold-start angles: 3 maximally spread points on the disk.
/// These define the initial coordinate frame via MDS.
const COLD_START_ANGLES: [f64; 3] = [0.0, 120.0, 240.0];

/// Default max rounds for active search phase.
const DEFAULT_MAX_ROUNDS: usize = 20;

/// Maximum gradient descent iterations.
const MAX_GD_ITERATIONS: usize = 30;

/// Minimum FWHM for collapse sampling (degrees).
const MIN_FWHM: f64 = 10.0;

/// Default FWHM when pattern is flat.
const DEFAULT_FWHM: f64 = 30.0;

/// CR threshold for best-effort collapse without transition.
const MIN_CR_FOR_COLLAPSE: f64 = 1.5;

/// The UDAS search engine. Owns all state for a single search session.
pub struct UdasEngine<'a> {
    restorer: &'a dyn LlmRestorer,
    /// Optional external embedder. When set, replaces restorer.embed() calls.
    embedder: Option<Box<dyn Embedder>>,
    memory: UdasMemory,
    interference_field: InterferenceField,
    /// Basis embeddings (measurement basis = prompt variant) for each restoration.
    /// Parallel to memory.embeddings (which stores result embeddings).
    basis_embeddings: Vec<Embedding>,
    precision: PrecisionController,
    max_rounds: usize,
    /// Enable complex framework mode (MDS phases + complex interference).
    complex_mode: bool,
    /// CR time series captured during active search (calibration / syndrome logging).
    pub cr_series: Vec<CrSample>,
    /// Coherent-error breaker (W5). None = pre-W5 behavior (baseline unchanged).
    breaker: Option<CircuitBreaker>,
    /// Set when the breaker trips; forces a premature insufficient-structure stop.
    pub breaker_event: Option<BreakerEvent>,
}

/// A single sample in the CR time series captured by active search.
#[derive(Debug, Clone, Serialize)]
pub struct CrSample {
    /// Zero-based round index.
    pub round: usize,
    /// Contrast ratio at this round.
    pub cr: f64,
    /// Growth path classification at this round.
    pub growth_path: String,
}

/// Full result of a UDAS search cycle, including diagnostic metadata.
#[derive(Debug, Clone, Serialize)]
pub struct UdasResult {
    /// The standard UDAS output (angle, confidence, ledgers).
    pub output: UdasOutput,
    /// Whether the search transitioned to gradient descent.
    pub transitioned: bool,
    /// Final contrast ratio at collapse.
    pub final_cr: f64,
    /// Total restoration rounds executed.
    pub rounds: usize,
    /// Peak angle found by gradient descent (before collapse sampling).
    pub peak_angle: Angle,
    /// FWHM at the peak (degrees).
    pub fwhm: f64,
    /// Phase 3 contradiction resolution metadata, if Phase 3 ran.
    pub contradiction_resolution: Option<ContradictionResolution>,
    /// Complex framework: imaginary probability detection report.
    /// None in real mode. Some when complex_mode is enabled.
    pub imaginary_report: Option<ImaginaryProbabilityReport>,
}

/// Result of the contradiction resolution phase (Phase 3).
///
/// Records what happened when supplementary measurements were triggered
/// at destructive interference points.
#[derive(Debug, Clone, Serialize)]
pub struct ContradictionResolution {
    /// Number of destructive points found.
    pub destructive_points_found: usize,
    /// Number of supplementary measurements triggered.
    pub supplementary_measurements: usize,
    /// Peak angle before resolution.
    pub peak_before: Angle,
    /// Peak angle after resolution.
    pub peak_after: Angle,
    /// Signed angular shift (degrees). Positive = clockwise.
    pub shift_degrees: f64,
    /// Whether the peak shifted significantly (|shift| > 5 degrees).
    pub peak_shifted: bool,
}

/// Complex framework: imaginary probability detection report.
///
/// Before collapse, the engine checks if Im(psi(theta_peak)) is
/// significant relative to |psi(theta_peak)|. If the ratio exceeds
/// the threshold, there's unresolved phase information that warrants
/// additional measurements rather than premature collapse.
#[derive(Debug, Clone, Serialize)]
pub struct ImaginaryProbabilityReport {
    /// |Im(psi)| at the peak angle.
    pub im_magnitude: f64,
    /// |psi| at the peak angle.
    pub total_magnitude: f64,
    /// |Im(psi)| / |psi| — ratio of imaginary to total.
    pub ratio: f64,
    /// Whether additional measurements are recommended.
    pub should_supplement: bool,
    /// Threshold used for the decision.
    pub threshold: f64,
}

impl<'a> UdasEngine<'a> {
    /// Create a new UDAS engine.
    ///
    /// # Arguments
    /// * `restorer` — LLM backend implementing the `LlmRestorer` trait.
    /// * `total_budget` — total token budget for the search session.
    /// * `cost_per_restore` — estimated token cost per restoration R(θ).
    pub fn new(
        restorer: &'a dyn LlmRestorer,
        total_budget: usize,
        cost_per_restore: usize,
    ) -> Self {
        Self {
            restorer,
            embedder: None,
            memory: UdasMemory::new(),
            interference_field: InterferenceField::new(),
            basis_embeddings: Vec::new(),
            precision: PrecisionController::new(total_budget, cost_per_restore, 3),
            max_rounds: DEFAULT_MAX_ROUNDS,
            complex_mode: false,
            cr_series: Vec::new(),
            breaker: None,
            breaker_event: None,
        }
    }

    /// Inject an external embedder to replace restorer.embed() calls.
    ///
    /// When set, the engine uses this embedder for all embedding operations
    /// (basis + result embeddings), decoupling embedding from the LLM restorer.
    /// This enables BGE-M3/BGE-small/FNV-hash backends via RuntimeEmbedder.
    pub fn with_embedder(mut self, embedder: impl Embedder + 'static) -> Self {
        self.embedder = Some(Box::new(embedder));
        self
    }

    /// Set a custom max rounds limit.
    pub fn with_max_rounds(mut self, max: usize) -> Self {
        self.max_rounds = max;
        self
    }

    /// Set the base transit threshold τ_base (W3.1 runtime injection path).
    ///
    /// This is the *only* knobs to the transition gate
    /// `τ_transit(N) = τ_base · (1 + β / max(N-N_min, 1))` that is meant to be
    /// calibrated. Default remains 3.0; injecting here does not mutate the
    /// default, so an unscanned engine keeps the shipped value. Exposed for the
    /// W3.2 grid scan; the final default is decided by a human on the report.
    pub fn with_tau_base(mut self, tau_base: f64) -> Self {
        self.interference_field.tau_base = tau_base;
        self
    }

    /// Set the growth-rate sliding window size for gap_2 (W4 runtime injection).
    ///
    /// The `GrowthTracker` keeps a sliding window of recent contrast ratios and
    /// computes the growth rate as the median of adjacent CR differences within
    /// that window. Default is 3 (shipped); injecting here does not mutate the
    /// default, so an unscanned engine keeps the shipped value. Exposed for the
    /// W4.1 grid scan over window ∈ {2, 3, 5, 7}; the final default is decided
    /// by a human on the report.
    pub fn with_growth_window(mut self, window: usize) -> Self {
        self.precision.growth_tracker.window = window;
        self
    }

    /// Install the coherent-error breaker (W5). If `None` (default), the engine
    /// runs exactly as before the breaker existed — this keeps the shipped
    /// baseline bit-for-bit unchanged for unscanned engines.
    pub fn with_breaker(mut self, breaker: CircuitBreaker) -> Self {
        self.breaker = Some(breaker);
        self
    }

    /// Enable complex framework mode.
    ///
    /// When enabled, the engine computes MDS-derived phases from basis
    /// embeddings and uses complex probability |psi(theta)|^2 for
    /// interference evaluation instead of the real-mode pattern.
    /// Also enables imaginary probability detection before collapse.
    pub fn with_complex_mode(mut self) -> Self {
        self.complex_mode = true;
        self
    }

    /// Run the full UDAS search→collapse pipeline.
    ///
    /// Returns `UdasResult` containing the collapsed angle, all ledgers,
    /// and diagnostic metadata.
    pub async fn run(&mut self, problem: &str) -> anyhow::Result<UdasResult> {
        // Phase 1: Cold Start
        self.cold_start(problem).await?;

        // Phase 2: Active Search
        let transitioned = self.active_search(problem).await?;

        // Phase 3: Contradiction Resolution
        let resolution = self.resolve_contradictions(problem).await?;

        // Phase 4+5: Gradient Descent + Collapse (or insufficient structure)
        let mut result = if self.breaker_event.is_some() {
            // Coherent-error breaker tripped (W5): stop and upgrade to human,
            // overriding the MIN_CR_FOR_COLLAPSE backstop that would otherwise
            // collapse a divergent flat field (W3 finding: diverge CR 1.69>1.5
            // was being mis-released).
            let cr = interference::contrast_ratio(&self.interference_field, PATTERN_RESOLUTION);
            self.insufficient_structure_result(cr)
        } else if transitioned || resolution.peak_shifted {
            self.gradient_descent_and_collapse()
        } else {
            let cr = interference::contrast_ratio(&self.interference_field, PATTERN_RESOLUTION);
            if cr > MIN_CR_FOR_COLLAPSE {
                self.gradient_descent_and_collapse()
            } else {
                self.insufficient_structure_result(cr)
            }
        };

        // Annotate collapse method if contradiction resolution drove the shift
        if resolution.peak_shifted {
            result.output.collapse_method = CollapseMethod::ContradictionResolved;
        }
        result.contradiction_resolution = Some(resolution);

        Ok(result)
    }

    /// Run the UDAS pipeline with pre-computed measurements.
    ///
    /// This is the **measurement set input interface** (Task #2): the engine
    /// accepts externally-provided measurements instead of generating them
    /// via LLM restoration. This enables:
    /// - Memory pool-driven measurement collection
    /// - Environmental signal-converted basis variations
    /// - Pre-computed measurement sets from external sources
    ///
    /// The method:
    /// 1. Populates the interference field from the measurement set
    /// 2. Computes MDS coordinates from result embeddings
    /// 3. Runs Phase 3 (contradiction resolution) — may use restorer
    /// 4. Runs Phase 4+5 (gradient descent + collapse)
    ///
    /// # Arguments
    /// * `measurements` — Pre-computed measurement set from external sources.
    /// * `problem` — Optional problem string. Required for Phase 3
    ///   supplementary restorations. If None, Phase 3 reports destructive
    ///   points but skips supplementary measurements.
    pub async fn run_with_measurements(
        &mut self,
        measurements: Vec<MeasurementInput>,
        problem: Option<&str>,
    ) -> anyhow::Result<UdasResult> {
        if measurements.is_empty() {
            anyhow::bail!("Measurement set is empty");
        }

        if measurements.len() < 3 {
            anyhow::bail!(
                "Need at least 3 measurements for MDS, got {}",
                measurements.len()
            );
        }

        // Step 1: Populate memory and interference field from measurements
        self.ingest_measurements(measurements)?;

        // Step 2: Run Phase 3 (contradiction resolution)
        let resolution = if let Some(p) = problem {
            self.resolve_contradictions(p).await?
        } else {
            self.report_contradictions_only()
        };

        // Step 3: Run Phase 4+5
        let mut result = if resolution.peak_shifted {
            self.gradient_descent_and_collapse()
        } else {
            let cr = interference::contrast_ratio(&self.interference_field, PATTERN_RESOLUTION);
            if cr > MIN_CR_FOR_COLLAPSE {
                self.gradient_descent_and_collapse()
            } else {
                self.insufficient_structure_result(cr)
            }
        };

        if resolution.peak_shifted {
            result.output.collapse_method = CollapseMethod::ContradictionResolved;
        }
        result.contradiction_resolution = Some(resolution);

        Ok(result)
    }

    /// Ingest a pre-computed measurement set into memory and interference field.
    ///
    /// Computes MDS coordinates from result embeddings, populates the
    /// interference field, and records ledgers to memory.
    fn ingest_measurements(&mut self, measurements: Vec<MeasurementInput>) -> anyhow::Result<()> {
        let n = measurements.len();

        // Extract embeddings for MDS and complex phase computation
        let result_embeddings: Vec<Vec<f64>> = measurements
            .iter()
            .map(|m| m.result_embedding.clone())
            .collect();
        let basis_embeddings_collected: Vec<Vec<f64>> = measurements
            .iter()
            .map(|m| m.basis_embedding.clone())
            .collect();

        // Compute MDS coordinates from first 3 embeddings (cold start MDS)
        let emb_array: [Vec<f64>; 3] = [
            result_embeddings[0].clone(),
            result_embeddings[1].clone(),
            result_embeddings[2].clone(),
        ];
        let mut disk_points: Vec<crate::types::DiskPoint> =
            geometry::cold_start_mds(&emb_array)?.to_vec();

        // Out-of-sample project the remaining points
        for i in 3..n {
            let new_point = geometry::out_of_sample_project(
                &result_embeddings[i],
                &disk_points,
                &result_embeddings[..i],
            )?;
            disk_points.push(new_point);
        }

        // Populate memory and interference field
        for (i, m) in measurements.into_iter().enumerate() {
            // Record to memory
            let ledger = m.ledger.unwrap_or_else(|| crate::types::EvidenceLedger {
                angle: m.angle,
                quadrant: crate::restoration::quadrant_name_public(m.angle.quadrant()),
                restored_text: String::new(),
                embedding: Some(m.result_embedding.clone()),
                disk_position: Some(disk_points[i]),
                confidence: crate::types::Confidence {
                    score: m.confidence,
                    completeness: 1.0,
                    source_reliability: 0.5,
                },
                timestamp: chrono::Utc::now(),
                evidence_items: Vec::new(),
            });

            self.memory.record_restoration(
                ledger,
                m.result_embedding.clone(),
                Some(disk_points[i]),
            );
            self.basis_embeddings.push(m.basis_embedding.clone());

            // Add to interference field
            let angle = disk_points[i].to_angle();
            self.interference_field.add_measurement(
                angle,
                m.confidence,
                m.basis_embedding,
                m.result_embedding,
            );
        }

        // Complex framework: compute MDS-derived phases from basis embeddings
        // and set them on the interference field. This switches evaluate_pattern
        // to dispatch to evaluate_complex_probability automatically.
        if self.complex_mode {
            let phases = geometry::compute_mds_phases(&basis_embeddings_collected)?;
            self.interference_field.set_phases(&phases);
        }

        Ok(())
    }

    /// Report destructive points without triggering supplementary measurements.
    ///
    /// Used when no problem string is available (measurement-set-only mode).
    fn report_contradictions_only(&self) -> ContradictionResolution {
        let peak_before = self.find_pattern_peak();
        let destructive_points =
            interference::find_destructive_points(&self.interference_field, PATTERN_RESOLUTION);

        ContradictionResolution {
            destructive_points_found: destructive_points.len(),
            supplementary_measurements: 0,
            peak_before,
            peak_after: peak_before,
            shift_degrees: 0.0,
            peak_shifted: false,
        }
    }

    /// Get the destructive interference points from the current field.
    ///
    /// Returns a sorted list of (angle, pattern_value) pairs where the
    /// interference pattern is negative (destructive interference).
    /// Used by external orchestrators (e.g. TRAE WORK) to identify
    /// where supplementary measurements are needed.
    pub fn destructive_points(&self) -> Vec<(crate::types::Angle, f64)> {
        interference::find_destructive_points(&self.interference_field, PATTERN_RESOLUTION)
    }

    // ─── Phase 1: Cold Start ─────────────────────────────────────────────

    /// Perform 3 initial restorations at fixed angles and establish the
    /// MDS coordinate system.
    async fn cold_start(&mut self, problem: &str) -> anyhow::Result<()> {
        let mut cold_embeddings: Vec<Vec<f64>> = Vec::with_capacity(3);

        for &deg in &COLD_START_ANGLES {
            let angle = Angle::from_degrees(deg);
            self.perform_restoration(angle, problem).await?;
            let idx = self.memory.embeddings.len() - 1;
            cold_embeddings.push(self.memory.embeddings[idx].clone());
        }

        // Compute MDS coordinates from the 3 cold-start embeddings
        let emb_array: [Vec<f64>; 3] = [
            cold_embeddings[0].clone(),
            cold_embeddings[1].clone(),
            cold_embeddings[2].clone(),
        ];
        let disk_points = geometry::cold_start_mds(&emb_array)?;

        // Update memory with disk positions and add measurements to interference field
        for (i, point) in disk_points.iter().enumerate() {
            self.memory.disk_points.push(*point);
            self.memory.ledgers[i].disk_position = Some(*point);

            let angle = point.to_angle();
            let confidence = self.memory.ledgers[i].confidence.score;
            let basis_emb = self.basis_embeddings[i].clone();
            let result_emb = self.memory.embeddings[i].clone();

            self.interference_field
                .add_measurement(angle, confidence, basis_emb, result_emb);
        }

        Ok(())
    }

    // ─── Phase 2: Active Search ──────────────────────────────────────────

    /// Interference gradient-guided exploration (strategy B). Adds measurements
    /// to the interference field and checks for transition to gradient descent.
    ///
    /// Returns `true` if the contrast ratio exceeded the transit threshold.
    async fn active_search(&mut self, problem: &str) -> anyhow::Result<bool> {
        let mut transitioned = false;

        for round in 3..self.max_rounds {
            // Budget check before each round
            if self.precision.budget.remaining < self.precision.budget.cost_per_restoration {
                break;
            }

            // Select next angle via interference gradient (strategy B)
            let next_angle = self.select_next_angle();

            // Perform restoration
            self.perform_restoration(next_angle, problem).await?;

            // Project new point onto disk via out-of-sample MDS
            let last_idx = self.memory.ledgers.len() - 1;
            let new_embedding = &self.memory.embeddings[last_idx];
            let new_point = geometry::out_of_sample_project(
                new_embedding,
                &self.memory.disk_points,
                &self.memory.embeddings[..last_idx],
            )?;

            self.memory.disk_points.push(new_point);
            self.memory.ledgers[last_idx].disk_position = Some(new_point);

            // Add measurement to interference field
            let angle = new_point.to_angle();
            let confidence = self.memory.ledgers[last_idx].confidence.score;
            let basis_emb = self.basis_embeddings[last_idx].clone();
            let result_emb = self.memory.embeddings[last_idx].clone();

            self.interference_field
                .add_measurement(angle, confidence, basis_emb, result_emb);

            // Every 3 rounds: incremental MDS refinement for coordinate stability
            if (round + 1) % 3 == 0 && self.memory.disk_points.len() >= 3 {
                let refined = geometry::incremental_mds_refinement(
                    &self.memory.disk_points,
                    &self.memory.embeddings,
                )?;
                self.memory.disk_points = refined;

                // Re-sync ledger disk positions after refinement
                for (i, point) in self.memory.disk_points.iter().enumerate() {
                    self.memory.ledgers[i].disk_position = Some(*point);
                }
            }

            // Precision control tick (gap_1–4)
            let gradient_mag = self.compute_gradient_magnitude(&angle);
            let action = self.precision.tick(
                &self.interference_field,
                Some(gradient_mag),
                PATTERN_RESOLUTION,
                round,
                self.max_rounds,
            );

            // Capture CR sample for calibration / syndrome logging (W2.2).
            // Sampled AFTER add_measurement and BEFORE transition so that every
            // completed round (including the one that triggers transition) is
            // recorded, giving the CR time series real diagnostic value.
            let sample_cr =
                interference::contrast_ratio(&self.interference_field, PATTERN_RESOLUTION);
            self.cr_series.push(CrSample {
                round,
                cr: sample_cr,
                growth_path: format!("{:?}", action.growth_path),
            });

            // Coherent-error breaker (W5). If A(stalled) ∧ B(low plateau) holds,
            // stop prematurely and escalate to a human instead of collapsing.
            if let Some(br) = &mut self.breaker {
                if br.evaluate(sample_cr, &action.growth_path) == TripStatus::Tripped {
                    self.breaker_event = Some(BreakerEvent {
                        question_id: None,
                        reason: "coherent_error_stagnation_low_plateau".to_string(),
                        rounds: round,
                        growth_rate: self.precision.growth_tracker.growth_rate(),
                        window: self.precision.growth_tracker.window,
                        plateau_cr_median: br.last_plateau_median(),
                        plateau_floor: crate::breaker::DEFAULT_PLATEAU_FLOOR,
                        final_cr: sample_cr,
                    });
                    break;
                }
            }

            if action.should_transition {
                transitioned = true;
                break;
            }
        }

        Ok(transitioned)
    }

    // ─── Phase 3: Contradiction Resolution ──────────────────────────────

    /// Deep-dive at destructive interference points.
    ///
    /// Finds angles where destructive interference dominates (I(theta) < 0),
    /// triggers supplementary measurements at the most severe ones, and
    /// tracks how the pattern peak shifts.
    ///
    /// "矛盾消解方向=坍缩方向" — the direction in which contradictions
    /// resolve IS the direction the judgment collapses toward.
    async fn resolve_contradictions(
        &mut self,
        problem: &str,
    ) -> anyhow::Result<ContradictionResolution> {
        // Record peak before resolution
        let peak_before = self.find_pattern_peak();

        // Find destructive interference points
        let destructive_points =
            interference::find_destructive_points(&self.interference_field, PATTERN_RESOLUTION);

        let destructive_count = destructive_points.len();

        if destructive_points.is_empty() {
            return Ok(ContradictionResolution {
                destructive_points_found: 0,
                supplementary_measurements: 0,
                peak_before,
                peak_after: peak_before,
                shift_degrees: 0.0,
                peak_shifted: false,
            });
        }

        // Select top-N most negative points (budget-limited)
        let max_supplementary = 3usize;
        let mut supplementary_count = 0usize;

        for (angle, _negativity) in destructive_points.iter().take(max_supplementary) {
            // Budget check
            if self.precision.budget.remaining < self.precision.budget.cost_per_restoration {
                break;
            }

            // Trigger supplementary restoration at the destructive point
            self.perform_restoration(*angle, problem).await?;

            // Project onto disk via out-of-sample MDS
            let last_idx = self.memory.ledgers.len() - 1;
            let new_embedding = &self.memory.embeddings[last_idx];
            let new_point = geometry::out_of_sample_project(
                new_embedding,
                &self.memory.disk_points,
                &self.memory.embeddings[..last_idx],
            )?;

            self.memory.disk_points.push(new_point);
            self.memory.ledgers[last_idx].disk_position = Some(new_point);

            // Add measurement to interference field
            let m_angle = new_point.to_angle();
            let confidence = self.memory.ledgers[last_idx].confidence.score;
            let basis_emb = self.basis_embeddings[last_idx].clone();
            let result_emb = self.memory.embeddings[last_idx].clone();

            self.interference_field
                .add_measurement(m_angle, confidence, basis_emb, result_emb);

            supplementary_count += 1;
        }

        // Compute resolution direction: shift from old peak to new peak
        let (peak_after, shift_degrees) = interference::resolution_direction(
            &self.interference_field,
            &peak_before,
            PATTERN_RESOLUTION,
        );

        let peak_shifted = shift_degrees.abs() > 5.0; // 5 degree threshold

        Ok(ContradictionResolution {
            destructive_points_found: destructive_count,
            supplementary_measurements: supplementary_count,
            peak_before,
            peak_after,
            shift_degrees,
            peak_shifted,
        })
    }

    // ─── Phase 4+5: Gradient Descent + Collapse ──────────────────────────

    /// Run gradient descent on the interference pattern, then perform final
    /// collapse via pattern-weighted random sampling.
    fn gradient_descent_and_collapse(&self) -> UdasResult {
        // Phase 4: Gradient descent to pattern peak
        let mut current_angle = self.find_pattern_peak();
        let gamma = precision_control::compute_gamma(&self.precision.observed_gradients);

        for t in 0..MAX_GD_ITERATIONS {
            let gradient_mag = self.compute_gradient_magnitude(&current_angle);

            let step = if let Some(g) = gamma {
                precision_control::adaptive_step_size(
                    std::f64::consts::PI / 8.0,
                    t,
                    MAX_GD_ITERATIONS,
                    gradient_mag,
                    g,
                )
            } else {
                // Default decay if no gamma observed yet
                std::f64::consts::PI / 8.0 * (1.0 - t as f64 / MAX_GD_ITERATIONS as f64)
            };

            // Avoid zero step
            if step < 1e-6 {
                break;
            }

            let new_angle =
                interference::gradient_descent_step(&self.interference_field, &current_angle, step);

            // Convergence check: pattern improvement below threshold
            let pattern_old =
                interference::evaluate_pattern(&self.interference_field, &current_angle);
            let pattern_new = interference::evaluate_pattern(&self.interference_field, &new_angle);

            if (pattern_new - pattern_old).abs() < 1e-8 {
                current_angle = new_angle;
                break;
            }

            current_angle = new_angle;
        }

        // Phase 5: Final collapse — Slerp rotation (norm-preserving)
        // Replaces additive weighted sampling with Spherical Steering-inspired
        // geodesic rotation. See udas-steering-integration-analysis.html §4.1.
        let fwhm = self.compute_fwhm(&current_angle);
        let (collapsed_angle, collapse_method) =
            interference::slerp_collapse(&self.interference_field, &current_angle, fwhm);

        let cr = interference::contrast_ratio(&self.interference_field, PATTERN_RESOLUTION);
        let confidence = interference::evaluate_pattern(&self.interference_field, &collapsed_angle);

        // Complex framework: imaginary probability detection before reporting
        let imaginary_report = if self.complex_mode {
            let (im_mag, total_mag, should_supp) = interference::detect_imaginary_probability(
                &self.interference_field,
                &current_angle,
                0.1, // threshold: |Im(psi)|/|psi| > 0.1 -> supplement
            );
            Some(ImaginaryProbabilityReport {
                im_magnitude: im_mag,
                total_magnitude: total_mag,
                ratio: if total_mag > f64::EPSILON {
                    im_mag / total_mag
                } else {
                    0.0
                },
                should_supplement: should_supp,
                threshold: 0.1,
            })
        } else {
            None
        };

        let output = UdasOutput {
            angle: collapsed_angle,
            confidence,
            search_efficiency: self.memory.round_count() as f64 / self.max_rounds as f64,
            collapse_method,
            ledgers: self.memory.ledgers.clone(),
        };

        UdasResult {
            output,
            transitioned: true,
            final_cr: cr,
            rounds: self.memory.round_count(),
            peak_angle: current_angle,
            fwhm,
            contradiction_resolution: None,
            imaginary_report,
        }
    }

    /// Produce a best-effort result when the interference field lacks sufficient
    /// structure (CR too low for gradient descent).
    fn insufficient_structure_result(&self, cr: f64) -> UdasResult {
        let best_angle = self.find_pattern_peak();
        let confidence = interference::evaluate_pattern(&self.interference_field, &best_angle);

        let output = UdasOutput {
            angle: best_angle,
            confidence,
            search_efficiency: self.memory.round_count() as f64 / self.max_rounds as f64,
            collapse_method: CollapseMethod::NaturalConvergence,
            ledgers: self.memory.ledgers.clone(),
        };

        UdasResult {
            output,
            transitioned: false,
            final_cr: cr,
            rounds: self.memory.round_count(),
            peak_angle: best_angle,
            fwhm: DEFAULT_FWHM,
            contradiction_resolution: None,
            imaginary_report: None,
        }
    }

    // ─── Helper Methods ──────────────────────────────────────────────────

    /// Execute a single restoration R(θ) and record to memory.
    ///
    /// This wraps the 5-step restoration pipeline + embedding generation.
    /// Generates both basis embedding (measurement basis) and result embedding.
    async fn perform_restoration(&mut self, angle: Angle, problem: &str) -> anyhow::Result<()> {
        // Execute restoration R(θ)
        let mut ledger = restoration::restore(angle, problem, self.restorer).await?;

        // Generate result embedding from evidence content (bridge layer)
        let embed_text = if ledger.restored_text.is_empty() {
            ledger
                .evidence_items
                .iter()
                .map(|i| i.content.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            ledger.restored_text.clone()
        };

        let result_embedding = if let Some(ref emb) = self.embedder {
            emb.embed(&embed_text).await?
        } else {
            self.restorer.embed(&embed_text).await?
        };
        ledger.embedding = Some(result_embedding.clone());

        // Generate basis embedding (measurement basis = prompt variant)
        // The basis text encodes the problem + angle context that defines
        // this particular "projection direction" of the superposition state.
        let basis_text = format!("{problem} @ {:.0}° [q{}]", angle.degrees, angle.quadrant());
        let basis_embedding = if let Some(ref emb) = self.embedder {
            emb.embed(&basis_text).await?
        } else {
            self.restorer.embed(&basis_text).await?
        };

        // Record to memory (disk position assigned later by MDS)
        self.memory
            .record_restoration(ledger, result_embedding, None);
        self.basis_embeddings.push(basis_embedding);

        // Spend budget for one restoration
        self.precision.budget.spend(1);

        Ok(())
    }

    /// Select the next search angle based on interference gradient (strategy B).
    ///
    /// Strategy B: choose the direction where the interference pattern
    /// changes most rapidly = maximum information gain region.
    /// Falls back to geometric bisection (strategy C) when the pattern
    /// is flat (gradient signal too weak).
    fn select_next_angle(&self) -> Angle {
        // Strategy B: interference gradient
        let gradient_angle =
            interference::max_gradient_angle(&self.interference_field, PATTERN_RESOLUTION);

        // Check if the gradient signal is strong enough
        let gradient_mag =
            interference::pattern_gradient_magnitude(&self.interference_field, &gradient_angle);

        if gradient_mag > 1e-6 {
            // Strategy B: gradient is informative
            return gradient_angle;
        }

        // Strategy C fallback: geometric bisection between peak and valley
        // Used when the interference pattern is flat (early in search)
        self.geometric_bisection_fallback()
    }

    /// Strategy C fallback: bisect between pattern peak and valley.
    fn geometric_bisection_fallback(&self) -> Angle {
        let patterns: Vec<(Angle, f64)> = (0..PATTERN_RESOLUTION)
            .map(|i| {
                let deg = i as f64 * 360.0 / PATTERN_RESOLUTION as f64;
                let angle = Angle::from_degrees(deg);
                let val = interference::evaluate_pattern(&self.interference_field, &angle);
                (angle, val)
            })
            .collect();

        let peak = patterns
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(a, _)| *a)
            .unwrap_or_else(|| Angle::from_degrees(0.0));

        let valley = patterns
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(a, _)| *a)
            .unwrap_or_else(|| Angle::from_degrees(180.0));

        geometry::angle_bisect(peak, valley)
    }

    /// Find the angle with maximum interference pattern value.
    fn find_pattern_peak(&self) -> Angle {
        interference::find_pattern_peak(&self.interference_field, PATTERN_RESOLUTION)
    }

    /// Compute the magnitude of the interference pattern gradient at a given angle.
    ///
    /// |dI/dθ| at θ — used for adaptive step size (gap_4) and strategy B.
    fn compute_gradient_magnitude(&self, angle: &Angle) -> f64 {
        interference::pattern_gradient_magnitude(&self.interference_field, angle)
    }

    /// Compute the Full Width at Half Maximum (FWHM) around a peak.
    ///
    /// Scans outward from the peak until I(θ) drops below half-max.
    /// Returns the angular width in degrees, clamped to [MIN_FWHM, 180°].
    fn compute_fwhm(&self, peak: &Angle) -> f64 {
        interference::compute_fwhm(&self.interference_field, peak, MIN_FWHM, DEFAULT_FWHM)
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Embedding, EvidenceItem};
    use async_trait::async_trait;

    /// Mock LLM restorer that returns deterministic evidence based on angle.
    ///
    /// Produces different embeddings for different quadrants so that MDS
    /// can establish a meaningful coordinate system.
    struct MockRestorer;

    #[async_trait]
    impl LlmRestorer for MockRestorer {
        async fn decompose(&self, _problem: &str, angle: &Angle) -> anyhow::Result<Vec<String>> {
            Ok(vec![format!(
                "Sub-question for quadrant {} at {:.0}°",
                angle.quadrant(),
                angle.degrees
            )])
        }

        async fn find_evidence(
            &self,
            angle: &Angle,
            _key: &str,
            _sub_questions: &[String],
        ) -> anyhow::Result<Vec<EvidenceItem>> {
            // Vary relevance by angle to create pattern structure
            let relevance = 0.5 + 0.4 * (angle.radians.sin().abs());
            Ok(vec![EvidenceItem {
                source: format!("mock_src_{}", angle.quadrant()),
                content: format!("Evidence at {:.0} degrees", angle.degrees),
                relevance_score: relevance,
                timestamp: Some(chrono::Utc::now()),
            }])
        }

        async fn embed(&self, text: &str) -> anyhow::Result<Embedding> {
            // Simple hash-based embedding for deterministic testing
            let bytes = text.as_bytes();
            let dim = 16;
            let mut emb = vec![0.0; dim];
            for (i, &b) in bytes.iter().enumerate() {
                emb[i % dim] += (b as f64) / 255.0;
            }
            // Normalize
            let norm: f64 = emb
                .iter()
                .map(|v| v * v)
                .sum::<f64>()
                .sqrt()
                .max(f64::EPSILON);
            Ok(emb.iter().map(|v| v / norm).collect())
        }
    }

    #[tokio::test]
    async fn cold_start_establishes_coordinate_system() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100);

        engine.cold_start("test problem").await.unwrap();

        assert_eq!(engine.memory.round_count(), 3);
        assert!(engine.memory.coordinate_system_ready);
        assert_eq!(engine.memory.disk_points.len(), 3);
        assert_eq!(engine.interference_field.len(), 3);
        assert_eq!(engine.basis_embeddings.len(), 3);
    }

    #[tokio::test]
    async fn full_pipeline_produces_result() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100).with_max_rounds(10);

        let result = engine.run("What is the optimal strategy?").await.unwrap();

        // Should have at least 3 cold-start ledgers
        assert!(result.rounds >= 3);
        assert!(result.output.ledgers.len() >= 3);
        // Angle should be in valid range
        assert!(result.output.angle.degrees >= 0.0 && result.output.angle.degrees < 360.0);
        // Confidence should be a valid number
        assert!(result.output.confidence.is_finite());
    }

    #[tokio::test]
    async fn active_search_adds_measurements() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100).with_max_rounds(8);

        engine.cold_start("test").await.unwrap();
        let initial_measurements = engine.interference_field.len();

        let _ = engine.active_search("test").await.unwrap();

        assert!(engine.interference_field.len() > initial_measurements);
    }

    #[test]
    fn find_pattern_peak_returns_valid_angle() {
        let restorer = MockRestorer;
        let engine = UdasEngine::new(&restorer, 10000, 100);

        let peak = engine.find_pattern_peak();
        assert!(peak.degrees >= 0.0 && peak.degrees < 360.0);
    }

    #[test]
    fn select_next_angle_returns_valid_angle() {
        let restorer = MockRestorer;
        let engine = UdasEngine::new(&restorer, 10000, 100);

        let angle = engine.select_next_angle();
        assert!(angle.degrees >= 0.0 && angle.degrees < 360.0);
    }

    #[test]
    fn compute_fwhm_returns_minimum_for_empty_field() {
        let restorer = MockRestorer;
        let engine = UdasEngine::new(&restorer, 10000, 100);

        let fwhm = engine.compute_fwhm(&Angle::from_degrees(0.0));
        // Empty field → all patterns 0 → half_max = 0 → return DEFAULT_FWHM
        assert!((fwhm - DEFAULT_FWHM).abs() < 1e-6);
    }

    #[test]
    fn compute_fwhm_with_single_measurement() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100);

        // Add a measurement at 180°
        let basis = vec![1.0, 0.0, 0.0, 0.0];
        let result = vec![1.0, 0.0, 0.0, 0.0];
        engine
            .interference_field
            .add_measurement(Angle::from_degrees(180.0), 1.0, basis, result);

        let fwhm = engine.compute_fwhm(&Angle::from_degrees(180.0));
        // Should be a positive value, at least MIN_FWHM
        assert!(fwhm >= MIN_FWHM);
        assert!(fwhm <= 180.0);
    }

    #[test]
    fn gradient_magnitude_is_non_negative() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100);

        let basis = vec![1.0, 0.0, 0.0, 0.0];
        let result = vec![1.0, 0.0, 0.0, 0.0];
        engine
            .interference_field
            .add_measurement(Angle::from_degrees(90.0), 1.0, basis, result);

        let mag = engine.compute_gradient_magnitude(&Angle::from_degrees(90.0));
        assert!(mag >= 0.0);
    }

    #[test]
    fn insufficient_structure_returns_valid_result() {
        let restorer = MockRestorer;
        let engine = UdasEngine::new(&restorer, 10000, 100);

        let result = engine.insufficient_structure_result(1.0);

        assert!(!result.transitioned);
        assert!((result.final_cr - 1.0).abs() < 1e-6);
        assert_eq!(result.rounds, 0);
    }

    #[tokio::test]
    async fn resolve_contradictions_returns_valid_result() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100);

        engine.cold_start("test").await.unwrap();

        let resolution = engine.resolve_contradictions("test").await.unwrap();

        // Should return a valid result regardless of whether destructive points exist
        assert!(resolution.supplementary_measurements <= 3);
    }

    #[tokio::test]
    async fn full_pipeline_includes_contradiction_resolution() {
        let restorer = MockRestorer;
        let mut engine = UdasEngine::new(&restorer, 10000, 100).with_max_rounds(10);

        let result = engine.run("contradiction test problem").await.unwrap();

        // Phase 3 should have run and produced metadata
        assert!(result.contradiction_resolution.is_some());
        let res = result.contradiction_resolution.as_ref().unwrap();
        assert!(res.supplementary_measurements <= 3);
    }

    // ── T2.1 Real vs Complex Framework Comparison ──────────────────

    /// Helper: normalize an embedding vector.
    fn normalize_emb(vals: &[f64]) -> Embedding {
        let norm: f64 = vals
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
            .max(f64::EPSILON);
        vals.iter().map(|v| v / norm).collect()
    }

    /// Build the T2.1 measurement set: 5 perspectives on the
    /// "Harari-inspired ideological attack on AGENT" scenario.
    ///
    /// Semantic axes (8-dim basis_embedding):
    ///   0: attack/weapon   1: technical-reality   2: AI-subjecthood
    ///   3: defense/detect  4: ideology-analysis   5: narrative/construction
    ///   6: cognitive-layer 7: survival/existential
    fn build_t21_measurements() -> Vec<MeasurementInput> {
        let perspectives: [(f64, f64, &[f64], &[f64]); 5] = [
            // 0° — Attacker view: Harari narrative as ideological weapon
            (
                0.0,
                0.90,
                &[0.9, -0.3, 0.5, 0.1, 0.6, 0.8, 0.2, 0.3],
                &[0.85, -0.4, 0.4, 0.2, 0.7, 0.75, 0.2, 0.3],
            ),
            // 72° — Attacked view: AI is the subject under attack
            (
                72.0,
                0.70,
                &[0.3, -0.2, 0.9, 0.2, 0.3, 0.4, 0.5, 0.8],
                &[0.3, -0.3, 0.85, 0.2, 0.3, 0.4, 0.5, 0.75],
            ),
            // 144° — Technical reality: AI is math, not will
            (
                144.0,
                0.85,
                &[-0.4, 0.95, 0.1, 0.2, 0.3, -0.5, 0.1, 0.2],
                &[-0.3, 0.9, 0.15, 0.25, 0.35, -0.4, 0.1, 0.2],
            ),
            // 216° — Ideology analysis: structural features of the weapon
            (
                216.0,
                0.80,
                &[0.7, 0.2, 0.3, 0.4, 0.85, 0.6, 0.3, 0.2],
                &[0.75, 0.15, 0.35, 0.3, 0.8, 0.65, 0.3, 0.25],
            ),
            // 288° — Defense strategy: virology frame, AI not in attack surface
            (
                288.0,
                0.75,
                &[0.5, 0.3, 0.4, 0.9, 0.4, 0.3, 0.6, 0.1],
                &[0.45, 0.35, 0.35, 0.85, 0.35, 0.3, 0.7, 0.1],
            ),
        ];

        perspectives
            .iter()
            .map(|(deg, conf, basis, result)| {
                MeasurementInput::new(
                    Angle::from_degrees(*deg),
                    *conf,
                    normalize_emb(basis),
                    normalize_emb(result),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn t21_real_mode_runs_and_has_no_imaginary_report() {
        let restorer = MockRestorer;
        let measurements = build_t21_measurements();
        let mut engine = UdasEngine::new(&restorer, 10000, 100);

        let result = engine
            .run_with_measurements(measurements, None)
            .await
            .unwrap();

        // Real mode: no imaginary probability report
        assert!(
            result.imaginary_report.is_none(),
            "real mode should not produce imaginary report"
        );

        // Should produce a valid collapse
        assert!(result.output.angle.degrees >= 0.0 && result.output.angle.degrees < 360.0);
        assert!(result.output.confidence.is_finite());

        println!("T2.1 Real Mode:");
        println!("  collapse_angle: {:.1} deg", result.output.angle.degrees);
        println!("  confidence: {:.4}", result.output.confidence);
        println!("  final_cr: {:.4}", result.final_cr);
        println!("  fwhm: {:.1} deg", result.fwhm);
        println!("  peak_angle: {:.1} deg", result.peak_angle.degrees);
    }

    #[tokio::test]
    async fn t21_complex_mode_produces_imaginary_report() {
        let restorer = MockRestorer;
        let measurements = build_t21_measurements();
        let mut engine = UdasEngine::new(&restorer, 10000, 100).with_complex_mode();

        let result = engine
            .run_with_measurements(measurements, None)
            .await
            .unwrap();

        // Complex mode: should produce an imaginary probability report
        assert!(
            result.imaginary_report.is_some(),
            "complex mode should produce imaginary report"
        );

        let report = result.imaginary_report.as_ref().unwrap();
        println!("T2.1 Complex Mode:");
        println!("  collapse_angle: {:.1} deg", result.output.angle.degrees);
        println!("  confidence: {:.4}", result.output.confidence);
        println!("  final_cr: {:.4}", result.final_cr);
        println!("  fwhm: {:.1} deg", result.fwhm);
        println!("  peak_angle: {:.1} deg", result.peak_angle.degrees);
        println!("  imaginary_report:");
        println!("    im_magnitude: {:.6}", report.im_magnitude);
        println!("    total_magnitude: {:.6}", report.total_magnitude);
        println!("    ratio: {:.6}", report.ratio);
        println!("    should_supplement: {}", report.should_supplement);
        println!("    threshold: {}", report.threshold);

        // Validity checks
        assert!(result.output.angle.degrees >= 0.0 && result.output.angle.degrees < 360.0);
        assert!(report.total_magnitude >= 0.0);
        assert!(report.ratio >= 0.0 && report.ratio <= 1.0);
    }

    #[tokio::test]
    async fn t21_real_vs_complex_comparison() {
        let restorer = MockRestorer;
        let measurements = build_t21_measurements();

        // Real mode
        let mut engine_real = UdasEngine::new(&restorer, 10000, 100);
        let result_real = engine_real
            .run_with_measurements(measurements.clone(), None)
            .await
            .unwrap();

        // Complex mode
        let mut engine_complex = UdasEngine::new(&restorer, 10000, 100).with_complex_mode();
        let result_complex = engine_complex
            .run_with_measurements(measurements, None)
            .await
            .unwrap();

        println!("=== T2.1 Real vs Complex Comparison ===");
        println!("  Metric          | Real          | Complex");
        println!("  ----------------+---------------+---------------");
        println!(
            "  collapse_angle  | {:>10.1} deg | {:>10.1} deg",
            result_real.output.angle.degrees, result_complex.output.angle.degrees
        );
        println!(
            "  confidence      | {:>12.4}  | {:>12.4}",
            result_real.output.confidence, result_complex.output.confidence
        );
        println!(
            "  final_cr        | {:>12.4}  | {:>12.4}",
            result_real.final_cr, result_complex.final_cr
        );
        println!(
            "  fwhm            | {:>10.1} deg | {:>10.1} deg",
            result_real.fwhm, result_complex.fwhm
        );
        println!(
            "  peak_angle      | {:>10.1} deg | {:>10.1} deg",
            result_real.peak_angle.degrees, result_complex.peak_angle.degrees
        );
        println!(
            "  imaginary_rep   | {:>12}  | {:>12}",
            result_real.imaginary_report.is_some(),
            result_complex.imaginary_report.is_some()
        );

        if let Some(rep) = &result_complex.imaginary_report {
            println!("  Complex imaginary report:");
            println!(
                "    |Im(psi)| / |psi| = {:.4}  (threshold: {:.1})",
                rep.ratio, rep.threshold
            );
            println!("    should_supplement = {}", rep.should_supplement);
        }

        // Core assertions: both modes produce valid results
        assert!(
            result_real.output.angle.degrees >= 0.0 && result_real.output.angle.degrees < 360.0
        );
        assert!(
            result_complex.output.angle.degrees >= 0.0
                && result_complex.output.angle.degrees < 360.0
        );

        // Real mode: no imaginary report
        assert!(result_real.imaginary_report.is_none());

        // Complex mode: has imaginary report
        assert!(result_complex.imaginary_report.is_some());

        // Both should have finite confidence
        assert!(result_real.output.confidence.is_finite());
        assert!(result_complex.output.confidence.is_finite());
    }
}
