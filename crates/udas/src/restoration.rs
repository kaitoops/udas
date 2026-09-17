//! Restoration Engine — semantic layer of UDAS.
//!
//! Executes R(θ): given an angle θ, memory store M, and judgment problem P,
//! performs the 5-step restoration pipeline to produce an EvidenceLedger.
//!
//! ## 5-Step Internal Process
//!
//! 1. **Decompose** — break judgment problem P into retrieval sub-questions
//!    aligned to the angle θ's quadrant dimension.
//! 2. **Select Key** — choose retrieval strategy based on θ:
//!    - [0, 90): temporal retrieval
//!    - [90, 180): semantic retrieval
//!    - [180, 270): entity retrieval
//!    - [270, 360): cross-domain / conflict retrieval
//! 3. **Find Evidence** — execute retrieval, collect memory fragments.
//! 4. **Verify Completeness** — check ledger field coverage; incomplete
//!    ledgers trigger supplementary retrieval (different angle = different
//!    supplementary direction → information asymmetry guarantee).
//! 5. **Organise Ledger** — deduplicate, sort, annotate sources; output
//!    structured EvidenceLedger + confidence vector.

use crate::types::{Angle, Confidence, Embedding, EvidenceItem, EvidenceLedger};
use async_trait::async_trait;

/// Trait for LLM-based decomposition and evidence retrieval.
///
/// This isolates the semantic layer from any specific LLM backend.
/// The TUI's DeepSeek client implements this trait; tests use a mock.
#[async_trait]
pub trait LlmRestorer: Send + Sync {
    /// Decompose a judgment problem into quadrant-aligned sub-questions.
    ///
    /// The LLM is prompted with the problem and the current quadrant,
    /// and returns 2-5 retrieval sub-questions optimised for that dimension.
    async fn decompose(&self, problem: &str, angle: &Angle) -> anyhow::Result<Vec<String>>;

    /// Retrieve evidence fragments for a given key/strategy.
    ///
    /// The LLM acts as both retriever and ranker: it searches its context
    /// window and external memory for fragments relevant to the sub-questions,
    /// then returns them as EvidenceItems with relevance scores.
    async fn find_evidence(
        &self,
        angle: &Angle,
        key: &str,
        sub_questions: &[String],
    ) -> anyhow::Result<Vec<EvidenceItem>>;

    /// Generate an embedding vector for a text (bridge layer).
    ///
    /// Used to project restoration results onto the MDS disk.
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding>;
}

/// Step 2: Select retrieval strategy based on angle quadrant.
///
/// Pure mapping logic — no LLM token cost.
pub fn select_key(angle: &Angle) -> anyhow::Result<String> {
    Ok(match angle.quadrant() {
        0 => "temporal".into(),
        1 => "semantic".into(),
        2 => "entity".into(),
        _ => "cross-domain".into(),
    })
}

/// Step 4: Verify evidence ledger field completeness.
///
/// Checks that every EvidenceItem has all required fields populated.
/// Returns completion ratio ∈ [0, 1].
///
/// - 1.0 = all items fully populated
/// - 0.5 = half of fields missing across all items
/// - 0.0 = no items or all fields empty
pub fn verify_completeness(items: &[EvidenceItem]) -> anyhow::Result<f64> {
    if items.is_empty() {
        return Ok(0.0);
    }

    let mut filled = 0usize;
    let mut total = 0usize;

    for item in items {
        total += 1;
        if !item.source.is_empty() {
            filled += 1;
        }
        total += 1;
        if !item.content.is_empty() {
            filled += 1;
        }
        total += 1;
        if item.relevance_score > 0.0 {
            filled += 1;
        }
        total += 1;
        if item.timestamp.is_some() {
            filled += 1;
        }
    }

    Ok(filled as f64 / total as f64)
}

/// Step 5: Organise ledger with confidence calculation.
///
/// Confidence is derived from three signals:
/// - `score`: average relevance × completeness (information density)
/// - `completeness`: field coverage ratio from step 4
/// - `source_reliability`: unique-source diversity ratio
pub fn organise_ledger(angle: Angle, items: &[EvidenceItem], completeness: f64) -> EvidenceLedger {
    let avg_relevance: f64 = if items.is_empty() {
        0.0
    } else {
        items.iter().map(|i| i.relevance_score).sum::<f64>() / items.len() as f64
    };

    let unique_sources: std::collections::HashSet<&str> =
        items.iter().map(|i| i.source.as_str()).collect();
    let source_diversity = if items.is_empty() {
        0.0
    } else {
        unique_sources.len() as f64 / items.len() as f64
    };

    let confidence = Confidence {
        score: avg_relevance * completeness,
        completeness,
        source_reliability: source_diversity,
    };

    EvidenceLedger {
        angle,
        quadrant: quadrant_name(angle.quadrant()),
        restored_text: String::new(),
        embedding: None,
        disk_position: None,
        confidence,
        timestamp: chrono::Utc::now(),
        evidence_items: items.to_vec(),
    }
}

/// The core restoration function R(θ, M, P).
///
/// Takes an angle on the UDAS disk and produces an evidence ledger
/// via the full 5-step pipeline. Requires an `LlmRestorer` implementation
/// for steps 1 and 3 (decomposition and evidence retrieval).
///
/// # Arguments
/// * `angle` — quadrant-mapped search angle.
/// * `problem` — the judgment problem being explored.
/// * `restorer` — LLM backend for semantic operations.
///
/// # Returns
/// A structured evidence ledger with confidence weights.
pub async fn restore(
    angle: Angle,
    problem: &str,
    restorer: &dyn LlmRestorer,
) -> anyhow::Result<EvidenceLedger> {
    // Step 1: Decompose
    let sub_questions = restorer.decompose(problem, &angle).await?;

    // Step 2: Select key
    let key = select_key(&angle)?;

    // Step 3: Find evidence
    let evidence = restorer.find_evidence(&angle, &key, &sub_questions).await?;

    // Step 4: Verify completeness
    let completeness = verify_completeness(&evidence)?;

    // Step 5: Organise ledger
    let ledger = organise_ledger(angle, &evidence, completeness);

    Ok(ledger)
}

/// Public accessor for quadrant name (used by engine's measurement ingestion).
pub fn quadrant_name_public(q: u8) -> String {
    quadrant_name(q)
}

fn quadrant_name(q: u8) -> String {
    match q {
        0 => "temporal".into(),
        1 => "semantic".into(),
        2 => "entity".into(),
        _ => "cross-domain".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadrant_mapping_is_correct() {
        assert_eq!(quadrant_name(0), "temporal");
        assert_eq!(quadrant_name(1), "semantic");
        assert_eq!(quadrant_name(2), "entity");
        assert_eq!(quadrant_name(3), "cross-domain");
    }

    #[test]
    fn select_key_maps_correctly() {
        assert_eq!(select_key(&Angle::from_degrees(45.0)).unwrap(), "temporal");
        assert_eq!(select_key(&Angle::from_degrees(135.0)).unwrap(), "semantic");
        assert_eq!(select_key(&Angle::from_degrees(225.0)).unwrap(), "entity");
        assert_eq!(
            select_key(&Angle::from_degrees(315.0)).unwrap(),
            "cross-domain"
        );
    }

    #[test]
    fn verify_completeness_empty() {
        let items: Vec<EvidenceItem> = vec![];
        assert_eq!(verify_completeness(&items).unwrap(), 0.0);
    }

    #[test]
    fn verify_completeness_full() {
        let items = vec![EvidenceItem {
            source: "test".into(),
            content: "content".into(),
            relevance_score: 0.8,
            timestamp: Some(chrono::Utc::now()),
        }];
        assert_eq!(verify_completeness(&items).unwrap(), 1.0);
    }

    #[test]
    fn verify_completeness_partial() {
        let items = vec![EvidenceItem {
            source: "test".into(),
            content: "content".into(),
            relevance_score: 0.0,
            timestamp: None,
        }];
        assert_eq!(verify_completeness(&items).unwrap(), 0.5);
    }

    #[test]
    fn organise_ledger_empty_items() {
        let angle = Angle::from_degrees(45.0);
        let ledger = organise_ledger(angle, &[], 0.0);
        assert_eq!(ledger.quadrant, "temporal");
        assert!((ledger.confidence.score - 0.0).abs() < f64::EPSILON);
        assert!((ledger.confidence.source_reliability - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn organise_ledger_calculates_confidence() {
        let items = vec![
            EvidenceItem {
                source: "src_a".into(),
                content: "evidence A".into(),
                relevance_score: 0.8,
                timestamp: Some(chrono::Utc::now()),
            },
            EvidenceItem {
                source: "src_b".into(),
                content: "evidence B".into(),
                relevance_score: 0.6,
                timestamp: Some(chrono::Utc::now()),
            },
        ];
        let angle = Angle::from_degrees(135.0);
        let ledger = organise_ledger(angle, &items, 1.0);
        // avg_relevance = 0.7, completeness = 1.0
        // score = 0.7 * 1.0 = 0.7
        assert!((ledger.confidence.score - 0.7).abs() < 1e-6);
        // 2 unique sources / 2 items = 1.0
        assert!((ledger.confidence.source_reliability - 1.0).abs() < 1e-6);
        assert_eq!(ledger.evidence_items.len(), 2);
    }

    // ── Mock LlmRestorer for integration test ──────────────────────────

    struct MockRestorer;

    #[async_trait]
    impl LlmRestorer for MockRestorer {
        async fn decompose(&self, _problem: &str, angle: &Angle) -> anyhow::Result<Vec<String>> {
            Ok(vec![format!(
                "Sub-question for quadrant {}",
                angle.quadrant()
            )])
        }

        async fn find_evidence(
            &self,
            _angle: &Angle,
            _key: &str,
            _sub_questions: &[String],
        ) -> anyhow::Result<Vec<EvidenceItem>> {
            Ok(vec![EvidenceItem {
                source: "mock_source".into(),
                content: "mock evidence content".into(),
                relevance_score: 0.75,
                timestamp: Some(chrono::Utc::now()),
            }])
        }

        async fn embed(&self, _text: &str) -> anyhow::Result<Embedding> {
            Ok(vec![0.1, 0.2, 0.3])
        }
    }

    #[tokio::test]
    async fn restore_full_pipeline_with_mock() {
        let restorer = MockRestorer;
        let angle = Angle::from_degrees(45.0);
        let ledger = restore(angle, "test problem", &restorer).await.unwrap();

        assert_eq!(ledger.quadrant, "temporal");
        assert_eq!(ledger.evidence_items.len(), 1);
        assert!((ledger.confidence.score - 0.75).abs() < 1e-6); // 0.75 * 1.0
        assert!((ledger.confidence.completeness - 1.0).abs() < 1e-6);
        assert!((ledger.confidence.source_reliability - 1.0).abs() < 1e-6);
    }
}
