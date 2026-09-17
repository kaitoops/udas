//! Dimension projector — unifies embeddings from different backends to a
//! fixed dimension for interference field computation.
//!
//! ## Why projection is needed
//!
//! Different embedders output different dimensions:
//! - FNV hash:     256-d
//! - BGE-small:     512-d
//! - BGE-M3:       1024-d
//!
//! The interference field's `cosine_sim` requires all vectors to have the
//! same dimension. We project everything to `UNIFIED_DIM` (512).
//!
//! ## Projection methods
//!
//! - `input_dim > output_dim`: Random Gaussian projection (Johnson-Lindenstrauss).
//!   The projection matrix is deterministic (fixed seed LCG), so the same
//!   input always maps to the same output. JL lemma guarantees pairwise
//!   distances are approximately preserved.
//! - `input_dim == output_dim`: Identity (no-op).
//! - `input_dim < output_dim`: Zero-padding (append zeros). This doesn't
//!   add information but ensures interface consistency.

use crate::embedder::Embedding;

/// Projects embeddings from any dimension to the unified UDAS dimension (512).
///
/// The projection matrix is fixed at construction time. Switching embedders
/// requires reconstructing the projector, or previously cached/projected
/// embeddings will be inconsistent.
pub struct DimensionProjector {
    input_dim: usize,
    output_dim: usize,
    /// Projection matrix: output_dim × input_dim (only used when input_dim > output_dim).
    /// Stored as flattened row-major.
    matrix: Vec<f64>,
}

/// Fixed seed for the LCG that generates the projection matrix.
/// "UDAS" in ASCII hex, ensures deterministic projections across runs.
const PROJECTION_SEED: u64 = 0x5544_4153;

impl DimensionProjector {
    /// Create a projector from `input_dim` to `output_dim`.
    ///
    /// Uses a fixed seed for deterministic output.
    pub fn new(input_dim: usize, output_dim: usize) -> Self {
        let matrix = if input_dim > output_dim {
            generate_gaussian_matrix(output_dim, input_dim, PROJECTION_SEED)
        } else {
            Vec::new()
        };

        Self {
            input_dim,
            output_dim,
            matrix,
        }
    }

    /// Create a projector that targets the unified UDAS dimension (512).
    pub fn to_unified(input_dim: usize) -> Self {
        Self::new(input_dim, crate::embedder::UNIFIED_DIM)
    }

    /// Project an embedding to the unified dimension.
    ///
    /// # Panics
    /// Panics if `embedding.len()` does not match `input_dim`.
    pub fn project(&self, embedding: &[f64]) -> Embedding {
        assert_eq!(
            embedding.len(),
            self.input_dim,
            "DimensionProjector: input dimension mismatch (expected {}, got {})",
            self.input_dim,
            embedding.len()
        );

        if self.input_dim == self.output_dim {
            return embedding.to_vec();
        }

        if self.input_dim > self.output_dim {
            // Matrix multiply: output = matrix × input
            // matrix is output_dim × input_dim, stored row-major
            let mut output = vec![0.0f64; self.output_dim];
            for i in 0..self.output_dim {
                let row_start = i * self.input_dim;
                for j in 0..self.input_dim {
                    output[i] += self.matrix[row_start + j] * embedding[j];
                }
            }
            // Re-normalize after projection (JL projection doesn't preserve norms)
            let norm: f64 = output.iter().map(|v| v * v).sum::<f64>().sqrt();
            if norm > f64::EPSILON {
                for v in &mut output {
                    *v /= norm;
                }
            }
            output
        } else {
            // Zero-padding: input_dim < output_dim
            let mut output = vec![0.0f64; self.output_dim];
            output[..self.input_dim].copy_from_slice(embedding);
            output
        }
    }

    /// The output (unified) dimension.
    pub fn output_dim(&self) -> usize {
        self.output_dim
    }

    /// The expected input dimension.
    pub fn input_dim(&self) -> usize {
        self.input_dim
    }
}

// ─── Deterministic Gaussian matrix generation ───────────────────────────

/// Generate a Gaussian random matrix with a deterministic LCG.
///
/// Entries are ~ N(0, 1/sqrt(rows)) to satisfy JL lemma scaling.
/// The matrix is `rows × cols` stored in row-major order.
fn generate_gaussian_matrix(rows: usize, cols: usize, seed: u64) -> Vec<f64> {
    let mut lcg = Lcg::new(seed);
    let scale = 1.0 / (rows as f64).sqrt();
    let mut matrix = Vec::with_capacity(rows * cols);
    for _ in 0..(rows * cols) {
        let g = lcg.gaussian() * scale;
        matrix.push(g);
    }
    matrix
}

/// Simple deterministic LCG (Linear Congruential Generator).
///
/// Uses Numerical Recipes constants for good statistical properties.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        // Numerical Recipes LCG parameters
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_f64(&mut self) -> f64 {
        let v = self.next_u64();
        // Map to [0, 1) using upper 53 bits
        (v >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Box-Muller transform for standard normal distribution N(0, 1).
    fn gaussian(&mut self) -> f64 {
        let u1 = self.next_f64().max(f64::MIN_POSITIVE);
        let u2 = self.next_f64();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = 2.0 * std::f64::consts::PI * u2;
        r * theta.cos()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_when_dims_match() {
        let proj = DimensionProjector::new(512, 512);
        let input: Vec<f64> = (0..512).map(|i| i as f64 / 512.0).collect();
        let output = proj.project(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn zero_padding_when_input_smaller() {
        let proj = DimensionProjector::new(256, 512);
        let input: Vec<f64> = (0..256).map(|i| i as f64 / 256.0).collect();
        let output = proj.project(&input);
        assert_eq!(output.len(), 512);
        // First 256 elements match input
        assert_eq!(&output[..256], &input[..]);
        // Last 256 elements are zero
        assert!(output[256..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn projection_when_input_larger() {
        let proj = DimensionProjector::new(1024, 512);
        let input: Vec<f64> = (0..1024).map(|i| i as f64 / 1024.0).collect();
        let output = proj.project(&input);
        assert_eq!(output.len(), 512);
        // Output should be normalized
        let norm: f64 = output.iter().map(|v| v * v).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9, "Output should be L2-normalized");
    }

    #[test]
    fn projection_is_deterministic() {
        let proj = DimensionProjector::new(1024, 512);
        let input: Vec<f64> = (0..1024).map(|i| i as f64).collect();
        let a = proj.project(&input);
        let b = proj.project(&input);
        assert_eq!(a, b);
    }

    #[test]
    fn different_projectors_same_result() {
        // Two projectors with the same dims should produce the same matrix
        let proj1 = DimensionProjector::new(1024, 512);
        let proj2 = DimensionProjector::new(1024, 512);
        let input: Vec<f64> = (0..1024).map(|i| i as f64).collect();
        assert_eq!(proj1.project(&input), proj2.project(&input));
    }

    #[test]
    fn to_unified_uses_512() {
        let proj = DimensionProjector::to_unified(256);
        assert_eq!(proj.output_dim(), 512);
        assert_eq!(proj.input_dim(), 256);
    }

    #[test]
    fn projection_preserves_similarity_approximately() {
        // JL lemma: similar inputs should produce similar projections.
        // Two nearly identical vectors should have high cosine similarity
        // after projection from 1024→512.
        let proj = DimensionProjector::new(1024, 512);

        let mut vec_a = vec![0.1f64; 1024];
        let mut vec_b = vec_a.clone();
        // Make them slightly different
        vec_b[0] = 0.2;
        vec_b[1] = 0.15;

        // Normalize inputs
        let norm_a: f64 = vec_a.iter().map(|v| v * v).sum::<f64>().sqrt();
        let norm_b: f64 = vec_b.iter().map(|v| v * v).sum::<f64>().sqrt();
        for v in &mut vec_a {
            *v /= norm_a;
        }
        for v in &mut vec_b {
            *v /= norm_b;
        }

        let proj_a = proj.project(&vec_a);
        let proj_b = proj.project(&vec_b);

        let cos_sim = proj_a
            .iter()
            .zip(&proj_b)
            .map(|(a, b)| a * b)
            .sum::<f64>();

        // Similar inputs should have high cosine similarity (> 0.95)
        assert!(
            cos_sim > 0.95,
            "JL projection should preserve similarity: cos_sim = {cos_sim}"
        );
    }

    #[test]
    #[should_panic(expected = "dimension mismatch")]
    fn wrong_input_dim_panics() {
        let proj = DimensionProjector::new(256, 512);
        let input = vec![0.0f64; 128]; // Wrong dimension
        proj.project(&input);
    }
}
