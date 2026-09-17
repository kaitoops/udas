//! UDAS CLI — LLM-free pure math computation tool.
//!
//! This binary provides the mathematical core of UDAS (interference field,
//! MDS, contradiction resolution, collapse) WITHOUT any LLM dependency.
//!
//! TRAE WORK (or any external orchestrator) provides:
//! - LLM restorations (decompose + find_evidence) — using its own token
//! - Text embeddings (via the `embed` subcommand or embed-service)
//!
//! This binary provides:
//! - `embed` — deterministic local embedding (FNV-1a 256-d)
//! - `compute` — interference-driven collapse from a measurement set JSON
//! - `similarity` — cosine similarity between two texts
//! - `embed-service` — persistent embedding server (start/stop/status)
//!
//! ## Orchestration Flow
//!
//! 1. TRAE WORK collects environmental signals
//! 2. TRAE WORK generates basis variations
//! 3. TRAE WORK performs LLM restorations (its own token)
//! 4. TRAE WORK calls `udas-cli embed` for each basis + result text
//! 5. TRAE WORK assembles measurement set JSON
//! 6. TRAE WORK calls `udas-cli compute` -> gets collapse + destructive points
//! 7. If destructive points found, TRAE WORK performs supplementary
//!    measurements at those angles, adds to measurement set, re-computes

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

mod introspect;

// --- NullRestorer: LlmRestorer impl that is never called ---

use deepseek_udas::restoration::LlmRestorer;
use deepseek_udas::types::{Angle, Embedding, EvidenceItem};
use udas_embedding::{Embedder, RuntimeEmbedder};

/// A no-op LLM restorer.
///
/// All methods return errors because they should NEVER be called.
/// When `run_with_measurements(measurements, None)` is used, the engine
/// takes the `report_contradictions_only()` path which does not invoke
/// any LLM methods.
struct NullRestorer;

#[async_trait::async_trait]
impl LlmRestorer for NullRestorer {
    async fn decompose(&self, _problem: &str, _angle: &Angle) -> anyhow::Result<Vec<String>> {
        bail!("NullRestorer: decompose called — this should not happen with problem=None")
    }

    async fn find_evidence(
        &self,
        _angle: &Angle,
        _key: &str,
        _sub_questions: &[String],
    ) -> anyhow::Result<Vec<EvidenceItem>> {
        bail!("NullRestorer: find_evidence called — this should not happen with problem=None")
    }

    async fn embed(&self, _text: &str) -> anyhow::Result<Embedding> {
        bail!("NullRestorer: embed called — use udas-cli embed subcommand instead")
    }
}


// --- CLI Structure ---

#[derive(Parser)]
#[command(name = "udas-cli")]
#[command(about = "UDAS pure math computation — no LLM dependency")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a deterministic 256-d embedding for the given text.
    ///
    /// Usage: udas-cli embed "text to embed"
    /// Or:    echo "text" | udas-cli embed -
    Embed {
        /// Text to embed. Use "-" to read from stdin.
        text: String,
        /// Use the remote embedding service instead of local embedder.
        #[arg(long)]
        remote: bool,
    },
    /// Compute interference-driven collapse from a measurement set JSON.
    ///
    /// Reads JSON from stdin.
    Compute {
        /// Enable complex framework mode (MDS phases + imaginary probability detection).
        #[arg(long)]
        complex: bool,
    },
    /// Compute cosine similarity between two texts.
    ///
    /// Usage: udas-cli similarity "text A" "text B"
    Similarity {
        /// First text.
        text_a: String,
        /// Second text.
        text_b: String,
        /// Use the remote embedding service instead of local embedder.
        #[arg(long)]
        remote: bool,
    },
    /// Persistent embedding service management.
    ///
    /// Start/stop a background server that loads the embedding model
    /// once and serves requests over IPC. Eliminates the 10+ minute
    /// cold start per invocation.
    EmbedService {
        #[command(subcommand)]
        action: EmbedServiceAction,
    },
    /// UDAS introspection system — CSL classification, hot file, timeline.
    Introspect {
        #[command(subcommand)]
        action: introspect::IntrospectAction,
    },
}

#[derive(Subcommand)]
enum EmbedServiceAction {
    /// Start the embedding service (loads model, listens for requests).
    ///
    /// This runs in the foreground. For background operation, use:
    ///   Start-Process udas-cli -ArgumentList "embed-service","start"
    Start {
        /// Backend mode: auto, bge-m3, bge-small, fnv.
        #[arg(long, default_value = "auto")]
        backend: String,
        /// Transport: local_socket or tcp.
        #[arg(long, default_value = "local_socket")]
        transport: String,
        /// Model directory (default: C:\Users\WIN10\udas-tui\models).
        #[arg(long)]
        model_dir: Option<String>,
    },
    /// Stop the running embedding service.
    Stop,
    /// Check if the embedding service is running and get server info.
    Status,
    /// Send a ping to the running service (health check).
    Ping,
}

// --- JSON I/O Structures ---

#[derive(Deserialize)]
struct MeasurementJson {
    angle_degrees: f64,
    confidence: f64,
    basis_embedding: Vec<f64>,
    result_embedding: Vec<f64>,
}

#[derive(Deserialize)]
struct ComputeInput {
    measurements: Vec<MeasurementJson>,
}

#[derive(Serialize)]
struct CollapseOutput {
    angle_degrees: f64,
    confidence: f64,
    search_efficiency: f64,
    collapse_method: String,
}

#[derive(Serialize)]
struct DiagnosticsOutput {
    rounds: usize,
    transitioned: bool,
    final_cr: f64,
    peak_angle_degrees: f64,
    fwhm: f64,
}

#[derive(Serialize)]
struct ContradictionOutput {
    destructive_points_found: usize,
    supplementary_measurements: usize,
    peak_before_degrees: f64,
    peak_after_degrees: f64,
    shift_degrees: f64,
    peak_shifted: bool,
    destructive_point_angles: Vec<f64>,
}

#[derive(Serialize)]
struct ComputeResult {
    collapse: CollapseOutput,
    diagnostics: DiagnosticsOutput,
    contradiction_resolution: ContradictionOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    imaginary_report: Option<ImaginaryReportOutput>,
}

#[derive(Serialize)]
struct ImaginaryReportOutput {
    im_magnitude: f64,
    total_magnitude: f64,
    ratio: f64,
    should_supplement: bool,
    threshold: f64,
}

// --- Main ---

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Embed { text, remote } => {
            let input = if text == "-" {
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf.trim().to_string()
            } else {
                text
            };

            let embedding = if remote {
                let config = udas_embed_service::ClientConfig::default();
                let embedder = udas_embed_service::RemoteEmbedder::connect(config).await?;
                eprintln!("[udas-cli] embedding backend: remote");
                embedder.embed(&input).await?
            } else {
                let embedder = RuntimeEmbedder::auto();
                eprintln!("[udas-cli] embedding backend: {}", embedder.backend_name());
                embedder.embed(&input).await?
            };
            let json = serde_json::to_string(&embedding)?;
            println!("{json}");
        }
        Command::Compute { complex } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;

            let compute_input: ComputeInput =
                serde_json::from_str(&input).context("Failed to parse measurement set JSON")?;

            if compute_input.measurements.is_empty() {
                bail!("Measurement set is empty");
            }
            if compute_input.measurements.len() < 3 {
                bail!(
                    "Need at least 3 measurements for MDS, got {}",
                    compute_input.measurements.len()
                );
            }

            use deepseek_udas::types::MeasurementInput;
            let measurements: Vec<MeasurementInput> = compute_input
                .measurements
                .into_iter()
                .map(|m| {
                    MeasurementInput::new(
                        Angle::from_degrees(m.angle_degrees),
                        m.confidence,
                        m.basis_embedding,
                        m.result_embedding,
                    )
                })
                .collect();

            let restorer = NullRestorer;
            let budget = measurements.len() * 2000;
            let mut engine = deepseek_udas::engine::UdasEngine::new(&restorer, budget, 2000);
            if complex {
                engine = engine.with_complex_mode();
            }

            let result = engine
                .run_with_measurements(measurements, None)
                .await
                .context("UDAS engine error")?;

            let destructive_angles: Vec<f64> = engine
                .destructive_points()
                .iter()
                .map(|(a, _)| a.degrees)
                .collect();

            let imaginary_report = result.imaginary_report.as_ref().map(|r| {
                ImaginaryReportOutput {
                    im_magnitude: r.im_magnitude,
                    total_magnitude: r.total_magnitude,
                    ratio: r.ratio,
                    should_supplement: r.should_supplement,
                    threshold: r.threshold,
                }
            });

            let output = ComputeResult {
                collapse: CollapseOutput {
                    angle_degrees: result.output.angle.degrees,
                    confidence: result.output.confidence,
                    search_efficiency: result.output.search_efficiency,
                    collapse_method: format!("{:?}", result.output.collapse_method),
                },
                diagnostics: DiagnosticsOutput {
                    rounds: result.rounds,
                    transitioned: result.transitioned,
                    final_cr: result.final_cr,
                    peak_angle_degrees: result.peak_angle.degrees,
                    fwhm: result.fwhm,
                },
                contradiction_resolution: {
                    let cr = result.contradiction_resolution.as_ref();
                    ContradictionOutput {
                        destructive_points_found: cr.map(|c| c.destructive_points_found).unwrap_or(0),
                        supplementary_measurements: cr.map(|c| c.supplementary_measurements).unwrap_or(0),
                        peak_before_degrees: cr.map(|c| c.peak_before.degrees).unwrap_or(0.0),
                        peak_after_degrees: cr.map(|c| c.peak_after.degrees).unwrap_or(0.0),
                        shift_degrees: cr.map(|c| c.shift_degrees).unwrap_or(0.0),
                        peak_shifted: cr.map(|c| c.peak_shifted).unwrap_or(false),
                        destructive_point_angles: destructive_angles,
                    }
                },
                imaginary_report,
            };

            let json = serde_json::to_string_pretty(&output)?;
            println!("{json}");
        }
        Command::Similarity { text_a, text_b, remote } => {
            let (emb_a, emb_b, backend_name) = if remote {
                let config = udas_embed_service::ClientConfig::default();
                let embedder = udas_embed_service::RemoteEmbedder::connect(config).await?;
                eprintln!("[udas-cli] embedding backend: remote");
                (embedder.embed(&text_a).await?, embedder.embed(&text_b).await?, "remote".to_string())
            } else {
                let embedder = RuntimeEmbedder::auto();
                let name = embedder.backend_name().to_string();
                eprintln!("[udas-cli] embedding backend: {}", name);
                (embedder.embed(&text_a).await?, embedder.embed(&text_b).await?, name)
            };

            let sim = cosine_similarity(&emb_a, &emb_b);

            let output = serde_json::json!({
                "similarity": sim,
                "backend": backend_name,
                "dim": emb_a.len(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::EmbedService { action } => {
            handle_embed_service(action).await?;
        }
        Command::Introspect { action } => {
            introspect::handle(action)?;
        }
    }

    Ok(())
}

// --- Embed Service Handlers ---

/// Handle embed-service subcommands.
async fn handle_embed_service(action: EmbedServiceAction) -> Result<()> {
    match action {
        EmbedServiceAction::Start {
            backend,
            transport,
            model_dir,
        } => {
            // Initialize tracing for server logs
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "udas_embed_service=info,udas_embedding=info".into()),
                )
                .init();

            let mut config = udas_embed_service::ServerConfig::default();

            // Parse backend mode
            config.backend = backend
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid backend '{backend}': {e}"))?;

            // Parse transport
            config.transport = transport
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid transport '{transport}': {e}"))?;

            // Override model dir if specified
            if let Some(dir) = model_dir {
                config.model_dir = PathBuf::from(dir);
            }

            eprintln!("[udas-cli] Starting embed service...");
            eprintln!("[udas-cli]   backend:   {}", config.backend);
            eprintln!("[udas-cli]   transport: {}", config.transport);
            eprintln!("[udas-cli]   model_dir: {}", config.model_dir.display());

            let server = Arc::new(udas_embed_service::EmbedServer::new(config)?);
            let info = server.info();
            eprintln!(
                "[udas-cli] Model loaded: backend={}, native_dim={}, output_dim={}",
                info.backend, info.native_dim, info.output_dim,
            );

            // Handle Ctrl+C for graceful shutdown
            let server_clone = Arc::clone(&server);
            tokio::spawn(async move {
                tokio::signal::ctrl_c()
                    .await
                    .expect("failed to listen for ctrl+c");
                eprintln!("[udas-cli] Ctrl+C received, shutting down...");
                server_clone.shutdown();
            });

            server.run().await?;
            eprintln!("[udas-cli] Embed service stopped.");
        }
        EmbedServiceAction::Stop => {
            stop_embed_service()?;
        }
        EmbedServiceAction::Status => {
            status_embed_service().await?;
        }
        EmbedServiceAction::Ping => {
            ping_embed_service().await?;
        }
    }
    Ok(())
}

/// PID file path: ~/.udas/embed-service.pid
fn pid_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".udas")
        .join("embed-service.pid")
}

/// Stop the running embedding service by reading the PID file and
/// terminating the process.
fn stop_embed_service() -> Result<()> {
    let pid_file = pid_file_path();

    if !pid_file.exists() {
        eprintln!("[udas-cli] No PID file found at {}. Service may not be running.", pid_file.display());
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_file)
        .context("Failed to read PID file")?;
    let pid: u32 = pid_str
        .trim()
        .parse()
        .context("Invalid PID in PID file")?;

    eprintln!("[udas-cli] Stopping embed service (PID: {pid})...");

    #[cfg(windows)]
    {
        let output = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .context("Failed to execute taskkill")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not found") || stderr.contains("No tasks") {
                eprintln!("[udas-cli] Process not found (may have already exited).");
            } else {
                eprintln!("[udas-cli] taskkill stderr: {stderr}");
            }
        } else {
            eprintln!("[udas-cli] Service stopped.");
        }
    }

    #[cfg(not(windows))]
    {
        let output = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .context("Failed to execute kill")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("[udas-cli] kill stderr: {stderr}");
        } else {
            eprintln!("[udas-cli] Service stopped.");
        }
    }

    // Remove PID file
    let _ = std::fs::remove_file(&pid_file);

    Ok(())
}

/// Check if the embedding service is running and print server info.
async fn status_embed_service() -> Result<()> {
    let pid_file = pid_file_path();

    if !pid_file.exists() {
        let output = serde_json::json!({
            "running": false,
            "reason": "no PID file"
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_file).unwrap_or_default();
    let pid: u32 = pid_str.trim().parse().unwrap_or(0);

    let config = udas_embed_service::ClientConfig::default();
    match udas_embed_service::EmbedClient::connect(config).await {
        Ok(client) => {
            match client.info().await {
                Ok(info) => {
                    let output = serde_json::json!({
                        "running": true,
                        "pid": pid,
                        "backend": info.backend,
                        "native_dim": info.native_dim,
                        "output_dim": info.output_dim,
                        "model_dir": info.model_dir,
                        "uptime_secs": info.uptime_secs,
                        "requests_served": info.requests_served,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                Err(e) => {
                    let output = serde_json::json!({
                        "running": false,
                        "pid": pid,
                        "error": format!("info request failed: {e}"),
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
            }
        }
        Err(e) => {
            let output = serde_json::json!({
                "running": false,
                "pid": pid,
                "error": format!("connect failed: {e}"),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// Send a ping to the running embedding service.
async fn ping_embed_service() -> Result<()> {
    let config = udas_embed_service::ClientConfig::default();
    match udas_embed_service::EmbedClient::connect(config).await {
        Ok(client) => {
            match client.ping().await {
                Ok(uptime) => {
                    let output = serde_json::json!({
                        "pong": true,
                        "uptime_secs": uptime,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                Err(e) => {
                    bail!("ping failed: {e}");
                }
            }
        }
        Err(e) => {
            bail!("connect failed: {e}");
        }
    }
    Ok(())
}

// --- Helper Functions ---

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    debug_assert_eq!(
        a.len(), b.len(),
        "embedding dimension mismatch: {} vs {}", a.len(), b.len()
    );

    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|y| y * y).sum::<f64>().sqrt();

    if norm_a < 1e-12 || norm_b < 1e-12 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}
