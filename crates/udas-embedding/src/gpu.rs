//! GPU state detection — checks NVIDIA GPU availability and VRAM usage.
//!
//! Uses `nvidia-smi` CLI (available with NVIDIA drivers) to query GPU
//! memory. On systems without NVIDIA GPU, returns `GpuState::Unavailable`.

use std::process::Command;

/// GPU availability state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuState {
    /// GPU is available with sufficient free VRAM.
    Available { free_mb: u32, total_mb: u32 },
    /// GPU exists but VRAM is insufficient (e.g., game is running).
    Busy { used_mb: u32, total_mb: u32 },
    /// No NVIDIA GPU or nvidia-smi not found.
    Unavailable,
}

/// Minimum free VRAM (in MB) required to run BGE-M3.
/// BGE-M3 FP16 needs ~9.8GB; we require 10GB free to be safe.
const MIN_FREE_VRAM_MB: u32 = 10_000;

impl GpuState {
    /// Detect current GPU state.
    pub fn detect() -> Self {
        match query_nvidia_smi() {
            Some((used_mb, total_mb)) => {
                let free_mb = total_mb.saturating_sub(used_mb);
                if free_mb >= MIN_FREE_VRAM_MB {
                    GpuState::Available { free_mb, total_mb }
                } else {
                    GpuState::Busy { used_mb, total_mb }
                }
            }
            None => GpuState::Unavailable,
        }
    }

    /// Returns true if GPU has enough free VRAM for BGE-M3.
    pub fn is_available(&self) -> bool {
        matches!(self, GpuState::Available { .. })
    }

    /// Human-readable description for logging.
    pub fn description(&self) -> String {
        match self {
            GpuState::Available { free_mb, total_mb } => {
                format!("GPU available: {free_mb}MB free / {total_mb}MB total")
            }
            GpuState::Busy { used_mb, total_mb } => {
                format!("GPU busy: {used_mb}MB used / {total_mb}MB total (need {MIN_FREE_VRAM_MB}MB free)")
            }
            GpuState::Unavailable => "No NVIDIA GPU detected".to_string(),
        }
    }
}

/// Query nvidia-smi for GPU memory usage.
/// Returns (used_mb, total_mb) if successful.
fn query_nvidia_smi() -> Option<(u32, u32)> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?;
    let parts: Vec<&str> = line.trim().split(',').collect();
    if parts.len() < 2 {
        return None;
    }

    let used_mb: u32 = parts[0].trim().parse().ok()?;
    let total_mb: u32 = parts[1].trim().parse().ok()?;
    Some((used_mb, total_mb))
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_state_detect_runs_without_crash() {
        // This will either detect a GPU or return Unavailable.
        // Either way, it shouldn't crash.
        let state = GpuState::detect();
        println!("GPU state: {}", state.description());
    }

    #[test]
    fn gpu_state_is_available_logic() {
        let available = GpuState::Available { free_mb: 11_000, total_mb: 12_000 };
        let busy = GpuState::Busy { used_mb: 11_500, total_mb: 12_000 };
        let unavailable = GpuState::Unavailable;

        assert!(available.is_available());
        assert!(!busy.is_available());
        assert!(!unavailable.is_available());
    }

    #[test]
    fn gpu_state_descriptions() {
        let available = GpuState::Available { free_mb: 10_500, total_mb: 12_000 };
        assert!(available.description().contains("10"));
        assert!(available.description().contains("available"));

        let busy = GpuState::Busy { used_mb: 11_000, total_mb: 12_000 };
        assert!(busy.description().contains("busy"));

        let unavailable = GpuState::Unavailable;
        assert!(unavailable.description().contains("No NVIDIA"));
    }
}
