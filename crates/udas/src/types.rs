//! UDAS (Unitary Disk Active Search) — common types.
//!
//! Core data structures shared across all UDAS modules.
//! Every type here stems from the three-layer architecture:
//!   Semantic(LLM restoration) → Bridge(embedding) → Geometry(pure math).

use serde::{Deserialize, Serialize};

/// An angle on the UDAS emergent disk [0°, 360°).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Angle {
    /// Degrees, clamped to [0.0, 360.0).
    pub degrees: f64,
    /// Radians, derived from degrees.
    pub radians: f64,
}

impl Angle {
    /// Create from degrees. Automatically normalises to [0, 360).
    pub fn from_degrees(deg: f64) -> Self {
        let d = deg.rem_euclid(360.0);
        Self {
            degrees: d,
            radians: d.to_radians(),
        }
    }

    /// Which quadrant does this angle fall in?
    ///   0 = [0, 90) temporal
    ///   1 = [90, 180) semantic
    ///   2 = [180, 270) entity
    ///   3 = [270, 360) cross-domain/conflict
    pub fn quadrant(&self) -> u8 {
        (self.degrees / 90.0).floor() as u8 % 4
    }
}

/// A 2-D point on the emergent MDS disk.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiskPoint {
    pub x: f64,
    pub y: f64,
}

impl DiskPoint {
    pub fn to_angle(&self) -> Angle {
        Angle::from_degrees(self.y.atan2(self.x).to_degrees())
    }

    /// Cosine distance to another point (1 - cos_sim).
    pub fn cos_distance(&self, other: &DiskPoint) -> f64 {
        let dot = self.x * other.x + self.y * other.y;
        let norm_a = (self.x.powi(2) + self.y.powi(2)).sqrt().max(f64::EPSILON);
        let norm_b = (other.x.powi(2) + other.y.powi(2)).sqrt().max(f64::EPSILON);
        1.0 - (dot / (norm_a * norm_b))
    }
}

/// An embedding vector produced by the bridge layer.
pub type Embedding = Vec<f64>;

/// A complex number for the classical complex interference framework.
///
/// In the complex framework, measurement amplitudes are complex:
///   αₖ = √cₖ · e^{iφₖ} = √cₖ · (cos φₖ + i·sin φₖ)
///
/// The interference pattern becomes:
///   ψ(θ) = Σₖ αₖ · K(θ-θₖ)   (complex amplitude)
///   I(θ) = |ψ(θ)|²             (probability = |amplitude|²)
///
/// The imaginary component of ψ captures phase information that the
/// real-number framework discards. This is NOT quantum computing —
/// C^n is just 2n real numbers. The upgrade is mathematical, not physical.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Complex {
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// Construct from polar coordinates: r·e^{iθ} = r·(cos θ + i·sin θ).
    pub fn from_polar(r: f64, theta: f64) -> Self {
        Self {
            re: r * theta.cos(),
            im: r * theta.sin(),
        }
    }

    /// Modulus |z| = √(re² + im²).
    pub fn norm(self) -> f64 {
        self.norm_sq().sqrt()
    }

    /// Squared modulus |z|² = re² + im².
    pub fn norm_sq(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// Complex conjugate z* = re - i·im.
    pub fn conjugate(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }

    /// Argument (phase) φ = atan2(im, re).
    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    /// Scalar multiplication (real scalar).
    pub fn scale(self, s: f64) -> Self {
        Self {
            re: self.re * s,
            im: self.im * s,
        }
    }
}

impl std::ops::Add for Complex {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }
}

impl std::ops::Mul for Complex {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }
}

/// The structured output of every restoration R(θ).
///
/// Each restoration generates an evidence ledger whose field-filling
/// pattern differs by angle quadrant — this asymmetry is the information
/// asymmetry guarantee of UDAS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceLedger {
    /// The angle θ at which this restoration was performed.
    pub angle: Angle,
    /// Quadrant name: temporal | semantic | entity | cross-domain.
    pub quadrant: String,
    /// The restored/reconstructed text output from the LLM.
    pub restored_text: String,
    /// Embedding vector of the restored text (bridge layer output).
    pub embedding: Option<Embedding>,
    /// Position on the disk after MDS projection.
    pub disk_position: Option<DiskPoint>,
    /// Confidence weight w_k for KDE density field.
    pub confidence: Confidence,
    /// Timestamp of restoration.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Source evidence items found during retrieval.
    pub evidence_items: Vec<EvidenceItem>,
}

/// A single piece of evidence retrieved during restoration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub source: String,
    pub content: String,
    pub relevance_score: f64,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

/// Confidence vector associated with a restoration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Confidence {
    /// Overall confidence score ∈ [0, 1].
    pub score: f64,
    /// Field-level completeness score.
    pub completeness: f64,
    /// Source reliability score.
    pub source_reliability: f64,
}

/// The 5-step restoration pipeline for R(θ, M, P).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestorationStep {
    Decompose = 1,
    SelectKey = 2,
    FindEvidence = 3,
    VerifyCompleteness = 4,
    OrganizeLedger = 5,
}

/// Precision tier for adaptive cost control (gap_3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrecisionTier {
    /// K_max rounds + natural FWHM convergence via gradient descent.
    High,
    /// K_max/2 rounds + density-weighted random convergence (skip gradient descent).
    Medium,
    /// 3 rounds cold-start + direct density-weighted random convergence.
    Low,
}

/// Growth path for dynamic K adjustment (gap_2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrowthPath {
    FastHit,
    NormalSearch,
    Stagnation,
}

/// Collapse method used in final convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollapseMethod {
    NaturalConvergence,
    WeightedRandom,
    /// Collapse direction was influenced by contradiction resolution (Phase 3).
    /// The peak shifted after supplementary measurements resolved destructive
    /// interference points — the shift direction IS the collapse direction.
    ContradictionResolved,
    /// Slerp (Spherical Linear Interpolation) collapse.
    /// Replaces additive weighted sampling with norm-preserving rotation
    /// along the geodesic between peak and secondary interference direction.
    /// Inspired by Spherical Steering (arXiv 2602.08169).
    SlerpRotation,
}

/// A pre-computed measurement input for the engine.
///
/// Used when measurements come from external sources (memory pool,
/// environmental signals) rather than LLM restoration. This enables
/// the engine to run on externally-provided measurement sets without
/// performing its own restorations.
#[derive(Debug, Clone)]
pub struct MeasurementInput {
    /// Angle on the UDAS disk where this measurement landed.
    pub angle: Angle,
    /// Confidence weight c_k in [0, 1].
    pub confidence: f64,
    /// Embedding of the measurement basis (prompt variant text).
    pub basis_embedding: Embedding,
    /// Embedding of the restoration result (evidence content).
    pub result_embedding: Embedding,
    /// Optional pre-computed evidence ledger.
    pub ledger: Option<EvidenceLedger>,
    /// MDS-derived phase for complex framework (None = real mode).
    pub phase: Option<f64>,
}

impl MeasurementInput {
    /// Create a new measurement input.
    pub fn new(
        angle: Angle,
        confidence: f64,
        basis_embedding: Embedding,
        result_embedding: Embedding,
    ) -> Self {
        Self {
            angle,
            confidence,
            basis_embedding,
            result_embedding,
            ledger: None,
            phase: None,
        }
    }

    /// Attach a pre-computed evidence ledger.
    pub fn with_ledger(mut self, ledger: EvidenceLedger) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Set the MDS-derived phase for complex framework mode.
    pub fn with_phase(mut self, phase: f64) -> Self {
        self.phase = Some(phase);
        self
    }
}

/// Full UDAS output after one complete search cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdasOutput {
    /// Final converged angle direction.
    pub angle: Angle,
    /// Confidence at the density peak.
    pub confidence: f64,
    /// Total search efficiency (visited points / total rounds).
    pub search_efficiency: f64,
    /// How the collapse was achieved.
    pub collapse_method: CollapseMethod,
    /// All evidence ledgers generated during the search.
    pub ledgers: Vec<EvidenceLedger>,
}
