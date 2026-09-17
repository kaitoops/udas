//! Mean pooling and L2 normalization for BGE model outputs.
//!
//! BGE series models use mean pooling over token embeddings weighted by
//! attention mask, followed by L2 normalization.

use crate::embedder::Embedding;

/// Apply mean pooling to last_hidden_state using attention mask.
///
/// # Arguments
/// * `hidden_state` - Flattened tensor [batch=1, seq_len, hidden_dim]
///   (batch dimension assumed to be 1)
/// * `seq_len` - Sequence length (number of tokens)
/// * `hidden_dim` - Hidden dimension (e.g., 1024 for BGE-M3)
/// * `attention_mask` - Attention mask, 1 for valid tokens, 0 for padding
pub fn mean_pool(
    hidden_state: &[f32],
    seq_len: usize,
    hidden_dim: usize,
    attention_mask: &[i64],
) -> Embedding {
    let mut pooled = vec![0.0f64; hidden_dim];
    let mut valid_count = 0usize;

    for i in 0..seq_len {
        if attention_mask.get(i).copied().unwrap_or(1) == 1 {
            valid_count += 1;
            let offset = i * hidden_dim;
            for j in 0..hidden_dim {
                pooled[j] += hidden_state[offset + j] as f64;
            }
        }
    }

    // Mean
    let count = valid_count.max(1) as f64;
    for v in &mut pooled {
        *v /= count;
    }

    pooled
}

/// L2 normalize an embedding vector in-place.
pub fn l2_normalize(vec: &mut [f64]) {
    let norm: f64 = vec.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > f64::EPSILON {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
}

/// Combined mean pooling + L2 normalization (the standard BGE post-processing).
pub fn mean_pool_and_normalize(
    hidden_state: &[f32],
    seq_len: usize,
    hidden_dim: usize,
    attention_mask: &[i64],
) -> Embedding {
    let mut pooled = mean_pool(hidden_state, seq_len, hidden_dim, attention_mask);
    l2_normalize(&mut pooled);
    pooled
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pool_basic() {
        // 2 tokens, 3 dims
        // Token 0: [1.0, 2.0, 3.0], mask=1
        // Token 1: [4.0, 5.0, 6.0], mask=1
        let hidden = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mask = vec![1i64, 1];
        let pooled = mean_pool(&hidden, 2, 3, &mask);
        // Mean: [(1+4)/2, (2+5)/2, (3+6)/2] = [2.5, 3.5, 4.5]
        assert!((pooled[0] - 2.5).abs() < 1e-6);
        assert!((pooled[1] - 3.5).abs() < 1e-6);
        assert!((pooled[2] - 4.5).abs() < 1e-6);
    }

    #[test]
    fn mean_pool_with_padding() {
        // 3 tokens, 2 dims, token 1 is padding
        let hidden = vec![1.0f32, 2.0, 10.0, 20.0, 3.0, 4.0];
        let mask = vec![1i64, 0, 1];
        let pooled = mean_pool(&hidden, 3, 2, &mask);
        // Only tokens 0 and 2 count: [(1+3)/2, (2+4)/2] = [2.0, 3.0]
        assert!((pooled[0] - 2.0).abs() < 1e-6);
        assert!((pooled[1] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_unit_vector() {
        let mut v = vec![3.0, 4.0];
        l2_normalize(&mut v);
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9);
        // [3,4] normalized = [0.6, 0.8]
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_zero_vector() {
        let mut v = vec![0.0, 0.0, 0.0];
        l2_normalize(&mut v);
        // Should remain zero (no division by zero)
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn mean_pool_and_normalize_combined() {
        let hidden = vec![1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mask = vec![1i64, 1];
        let result = mean_pool_and_normalize(&hidden, 2, 3, &mask);
        let norm: f64 = result.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9);
    }
}
