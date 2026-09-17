//! BGE-small-zh ONNX embedder — CPU-only semantic embedding (512-d).
//!
//! Used as fallback when GPU is busy (e.g., during gaming).

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use ndarray::Array2;
use ort::inputs;
use ort::session::{Session, builder::GraphOptimizationLevel};
use ort::value::Tensor;

use crate::embedder::{Embedder, Embedding};
use crate::pooling::mean_pool_and_normalize;

/// BGE-small-zh native output dimension.
const BGE_SMALL_DIM: usize = 512;

/// BGE-small-zh ONNX embedder (CPU only).
pub struct BgeSmallEmbedder {
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
}

impl BgeSmallEmbedder {
    /// Create a new BGE-small embedder (CPU only).
    pub fn new(model_dir: &Path) -> Result<Self> {
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() {
            anyhow::bail!(
                "BGE-small ONNX model not found at {}. Run convert_bge_m3.py first.",
                model_path.display()
            );
        }

        use ort::ep::CPU;

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow::anyhow!("ort optimization: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| anyhow::anyhow!("ort threads: {e}"))?
            .with_execution_providers([CPU::default().build()])
            .map_err(|e| anyhow::anyhow!("ort EP: {e}"))?
            .commit_from_file(&model_path)
            .with_context(|| format!("Failed to load ONNX model from {}", model_path.display()))?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to load tokenizer from {}: {e}",
                tokenizer_path.display()
            )
        })?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    /// Synchronous embedding.
    pub fn embed_sync(&self, text: &str) -> Result<Embedding> {
        self.run_inference(text)
    }

    fn run_inference(&self, text: &str) -> Result<Embedding> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenizer failed to encode text: {e}"))?;

        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();
        let seq_len = input_ids.len();

        let input_ids_arr: Array2<i64> =
            Array2::from_shape_vec((1, seq_len), input_ids.iter().map(|&v| v as i64).collect())?;
        let attention_arr: Array2<i64> = Array2::from_shape_vec(
            (1, seq_len),
            attention_mask.iter().map(|&v| v as i64).collect(),
        )?;

        // ort 2.0: inputs! returns SessionInputs directly, no ?
        let inputs = inputs![
            "input_ids" => Tensor::from_array(input_ids_arr)?,
            "attention_mask" => Tensor::from_array(attention_arr)?,
        ];

        // ort 2.0: run() requires &mut self — use Mutex
        let mut session = self
            .session
            .lock()
            .map_err(|e| anyhow::anyhow!("Session mutex poisoned: {e}"))?;
        let outputs = session.run(inputs)?;

        // ort 2.0: try_extract_tensor returns (&Shape, &[T])
        let output_value = &outputs["last_hidden_state"];
        let (shape, hidden_data) = output_value.try_extract_tensor::<f32>()?;
        let shape_ref: &[i64] = &**shape;
        let (s_len, h_dim) = (shape_ref[1] as usize, shape_ref[2] as usize);

        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&v| v as i64).collect();

        let embedding = mean_pool_and_normalize(hidden_data, s_len, h_dim, &attention_mask_i64);

        Ok(embedding)
    }
}

/// Async wrapper for BGE-small embedding using Arc.
pub struct BgeSmallEmbedderAsync(std::sync::Arc<BgeSmallEmbedder>);

impl BgeSmallEmbedderAsync {
    pub fn new(model_dir: &Path) -> Result<Self> {
        Ok(Self(std::sync::Arc::new(BgeSmallEmbedder::new(model_dir)?)))
    }
}

#[async_trait]
impl Embedder for BgeSmallEmbedderAsync {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let inner = self.0.clone();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || inner.embed_sync(&text))
            .await
            .context("BGE-small spawn_blocking task panicked")?
    }

    fn native_dim(&self) -> usize {
        BGE_SMALL_DIM
    }

    fn name(&self) -> &str {
        "bge-small-zh-cpu"
    }
}
