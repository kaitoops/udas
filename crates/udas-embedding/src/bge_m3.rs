//! BGE-M3 ONNX embedder — GPU-accelerated semantic embedding (1024-d).
//!
//! Uses the `ort` crate to load BGE-M3 ONNX model with CUDA execution
//! provider. Falls back to CPU if CUDA is unavailable.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use ndarray::Array2;
use ort::inputs;
use ort::session::{Session, builder::GraphOptimizationLevel};
use ort::value::Tensor;

use crate::embedder::{Embedder, Embedding};
use crate::gpu::GpuState;
use crate::pooling::mean_pool_and_normalize;

/// BGE-M3 native output dimension.
const BGE_M3_DIM: usize = 1024;

/// BGE-M3 ONNX embedder with GPU acceleration.
pub struct BgeM3Embedder {
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    use_cuda: bool,
}

impl BgeM3Embedder {
    /// Create a new BGE-M3 embedder, preferring CUDA if GPU is available.
    pub fn new(model_dir: &Path) -> Result<Self> {
        let gpu_state = GpuState::detect();
        tracing::info!("BGE-M3 init: {}", gpu_state.description());

        let use_cuda = gpu_state.is_available();
        Self::new_with_backend(model_dir, use_cuda)
    }

    /// Create with explicit backend choice.
    fn new_with_backend(model_dir: &Path, use_cuda: bool) -> Result<Self> {
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() {
            anyhow::bail!(
                "BGE-M3 ONNX model not found at {}. Run convert_bge_m3.py first.",
                model_path.display()
            );
        }

        let mut builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow::anyhow!("ort optimization: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| anyhow::anyhow!("ort threads: {e}"))?;

        if use_cuda {
            use ort::ep::CUDA;
            builder = builder
                .with_execution_providers([
                    CUDA::default().build(),
                    ort::ep::CPU::default().build(),
                ])
                .map_err(|e| anyhow::anyhow!("ort EP: {e}"))?;
            tracing::info!("BGE-M3: using CUDA execution provider");
        } else {
            use ort::ep::CPU;
            builder = builder
                .with_execution_providers([CPU::default().build()])
                .map_err(|e| anyhow::anyhow!("ort EP: {e}"))?;
            tracing::info!("BGE-M3: using CPU execution provider");
        }

        let session = builder
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
            use_cuda,
        })
    }

    /// Create with CUDA explicitly (fails if CUDA unavailable).
    pub fn new_with_cuda(model_dir: &Path) -> Result<Self> {
        Self::new_with_backend(model_dir, true)
    }

    /// Create with CPU explicitly.
    pub fn new_cpu(model_dir: &Path) -> Result<Self> {
        Self::new_with_backend(model_dir, false)
    }

    /// Whether this instance is using CUDA.
    pub fn is_using_cuda(&self) -> bool {
        self.use_cuda
    }

    /// Synchronous embedding (for non-async contexts).
    pub fn embed_sync(&self, text: &str) -> Result<Embedding> {
        self.run_inference(text)
    }

    /// Run ONNX inference and return the embedding.
    fn run_inference(&self, text: &str) -> Result<Embedding> {
        // 1. Tokenize
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenizer failed to encode text: {e}"))?;

        let input_ids = encoding.get_ids();
        let attention_mask = encoding.get_attention_mask();
        let seq_len = input_ids.len();

        // 2. Convert to ndarray
        let input_ids_arr: Array2<i64> =
            Array2::from_shape_vec((1, seq_len), input_ids.iter().map(|&v| v as i64).collect())?;
        let attention_arr: Array2<i64> = Array2::from_shape_vec(
            (1, seq_len),
            attention_mask.iter().map(|&v| v as i64).collect(),
        )?;

        // 3. ONNX inference (ort 2.0: inputs! returns SessionInputs directly, no ?)
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

        // 4. Extract last_hidden_state (ort 2.0: try_extract_tensor returns (&Shape, &[T]))
        let output_value = &outputs["last_hidden_state"];
        let (shape, hidden_data) = output_value.try_extract_tensor::<f32>()?;
        let shape_ref = shape;
        let (s_len, h_dim) = (shape_ref[1] as usize, shape_ref[2] as usize);

        // 5. Mean pooling + L2 normalize
        let attention_mask_i64: Vec<i64> = attention_mask.iter().map(|&v| v as i64).collect();

        let embedding = mean_pool_and_normalize(hidden_data, s_len, h_dim, &attention_mask_i64);

        Ok(embedding)
    }
}

#[async_trait]
impl Embedder for BgeM3Embedder {
    async fn embed(&self, _text: &str) -> Result<Embedding> {
        tokio::task::spawn_blocking(move || {
            // BgeM3Embedder is not Send when wrapped in Arc due to Mutex<Session>
            // This is handled by BgeM3EmbedderAsync wrapper below.
            // For direct use, call embed_sync() from a blocking context.
            unreachable!("Use BgeM3EmbedderAsync for async access, or embed_sync for sync")
        })
        .await
        .map_err(|e| anyhow::anyhow!("BGE-M3 task panicked: {e}"))?
    }

    fn native_dim(&self) -> usize {
        BGE_M3_DIM
    }

    fn name(&self) -> &str {
        if self.use_cuda {
            "bge-m3-cuda"
        } else {
            "bge-m3-cpu"
        }
    }
}

/// Async wrapper for BGE-M3 embedding using Arc.
pub struct BgeM3EmbedderAsync(std::sync::Arc<BgeM3Embedder>);

impl BgeM3EmbedderAsync {
    pub fn new(model_dir: &Path) -> Result<Self> {
        Ok(Self(std::sync::Arc::new(BgeM3Embedder::new(model_dir)?)))
    }

    pub fn new_with_cuda(model_dir: &Path) -> Result<Self> {
        Ok(Self(std::sync::Arc::new(BgeM3Embedder::new_with_cuda(
            model_dir,
        )?)))
    }
}

#[async_trait]
impl Embedder for BgeM3EmbedderAsync {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let inner = self.0.clone();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || inner.embed_sync(&text))
            .await
            .context("BGE-M3 spawn_blocking task panicked")?
    }

    fn native_dim(&self) -> usize {
        BGE_M3_DIM
    }

    fn name(&self) -> &str {
        self.0.name()
    }
}
