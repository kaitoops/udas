//! Memory Management — embedding storage and coordinate tracking.
//!
//! Persists the state of UDAS across search cycles:
//! - Embedding vectors for all visited restoration points
//! - Disk coordinates (MDS-projected positions)
//! - Evidence ledgers for each R(θ)
//! - Coordinate system metadata (for cold-start and incremental refinement)
//!
//! ## Design
//!
//! All storage uses an in-memory Vec-based store for phase 1 prototype.
//! Phase 2 can replace with SQLite (via rusqlite, already in workspace deps).

use crate::types::{Angle, DiskPoint, Embedding, EvidenceLedger};

/// Stores all traversal state for a single UDAS search session.
pub struct UdasMemory {
    /// All evidence ledgers indexed by angle (restoration order).
    pub ledgers: Vec<EvidenceLedger>,
    /// Embedding vectors corresponding to each ledger.
    pub embeddings: Vec<Embedding>,
    /// Disk positions after MDS projection.
    pub disk_points: Vec<DiskPoint>,
    /// Current cold-start state: 0 = not started, 3 = complete.
    pub cold_start_count: u8,
    /// Has the coordinate system been initialised?
    pub coordinate_system_ready: bool,
}

impl UdasMemory {
    /// Create a fresh, empty memory store.
    pub fn new() -> Self {
        Self {
            ledgers: Vec::new(),
            embeddings: Vec::new(),
            disk_points: Vec::new(),
            cold_start_count: 0,
            coordinate_system_ready: false,
        }
    }

    /// Store a completed restoration R(θ) with its embedding and disk position.
    pub fn record_restoration(
        &mut self,
        ledger: EvidenceLedger,
        embedding: Embedding,
        disk_point: Option<DiskPoint>,
    ) {
        self.ledgers.push(ledger);
        self.embeddings.push(embedding);
        if let Some(pt) = disk_point {
            self.disk_points.push(pt);
        }
        if self.cold_start_count < 3 {
            self.cold_start_count += 1;
            if self.cold_start_count == 3 {
                self.coordinate_system_ready = true;
            }
        }
    }

    /// Get the most recent N ledgers for growth-rate analysis (gap_2).
    pub fn recent_ledgers(&self, n: usize) -> &[EvidenceLedger] {
        let start = self.ledgers.len().saturating_sub(n);
        &self.ledgers[start..]
    }

    /// Total number of restoration rounds completed.
    pub fn round_count(&self) -> usize {
        self.ledgers.len()
    }

    /// Get all angles visited so far, in restoration order.
    pub fn visited_angles(&self) -> Vec<Angle> {
        self.ledgers.iter().map(|l| l.angle).collect()
    }

    /// Clear all data (for restarting a search).
    pub fn reset(&mut self) {
        self.ledgers.clear();
        self.embeddings.clear();
        self.disk_points.clear();
        self.cold_start_count = 0;
        self.coordinate_system_ready = false;
    }
}

impl Default for UdasMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Persist memory to disk (phase 2: SQLite).
///
/// For phase 1 prototype, returns the JSON-serialisable structure.
pub fn persist_ledgers(ledgers: &[EvidenceLedger]) -> anyhow::Result<String> {
    serde_json::to_string_pretty(ledgers)
        .map_err(|e| anyhow::anyhow!("serialization failed: {}", e))
}

/// Load memory from disk (phase 2: SQLite).
pub fn load_ledgers(json: &str) -> anyhow::Result<Vec<EvidenceLedger>> {
    serde_json::from_str(json).map_err(|e| anyhow::anyhow!("deserialization failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Confidence, EvidenceItem};

    fn dummy_ledger(angle_deg: f64) -> EvidenceLedger {
        EvidenceLedger {
            angle: Angle::from_degrees(angle_deg),
            quadrant: "temporal".into(),
            restored_text: String::new(),
            embedding: None,
            disk_position: None,
            confidence: Confidence {
                score: 0.5,
                completeness: 0.5,
                source_reliability: 0.5,
            },
            timestamp: chrono::Utc::now(),
            evidence_items: Vec::new(),
        }
    }

    #[test]
    fn cold_start_triggers_at_3() {
        let mut mem = UdasMemory::new();
        assert!(!mem.coordinate_system_ready);
        for i in 0..3 {
            mem.record_restoration(dummy_ledger(i as f64), vec![1.0, 2.0, 3.0], None);
        }
        assert!(mem.coordinate_system_ready);
        assert_eq!(mem.round_count(), 3);
    }

    #[test]
    fn recent_ledgers_returns_correct_window() {
        let mut mem = UdasMemory::new();
        for i in 0..5 {
            mem.record_restoration(dummy_ledger(i as f64), vec![1.0], None);
        }
        let recent = mem.recent_ledgers(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].angle.degrees, 2.0);
        assert_eq!(recent[2].angle.degrees, 4.0);
    }
}
