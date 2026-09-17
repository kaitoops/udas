//! UDAS Bridge — connects the `deepseek-udas` semantic layer to the
//! TUI's `DeepSeekClient` LLM backend.
//!
//! Implements [`LlmRestorer`] for [`DeepSeekClient`] so that UDAS's
//! `restore()` pipeline can decompose problems, retrieve evidence, and
//! generate embeddings via the configured DeepSeek API.
//!
//! ## Embedding Strategy
//!
//! Two modes controlled by `EmbeddingMode` on `DeepSeekClient`:
//!
//! - **Fnv** (default): Deterministic FNV-1a 256-d hash. Zero model loading,
//!   instant startup. Preserves UDAS geometry contract without extra services.
//! - **BgeM3**: Semantic 1024-d embedding via remote BGE-M3 service.
//!   Requires `udas-cli embed-service start`. Falls back to FNV if service
//!   is unavailable (with warning log).
//!
//! Toggle at runtime via `/bge` command or `UDAS_EMBEDDING_MODE` env var.

use crate::client::{DeepSeekClient, EmbeddingMode};
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};
use async_trait::async_trait;
use deepseek_udas::restoration::LlmRestorer;
use deepseek_udas::types::{Angle, Embedding, EvidenceItem};
use udas_embed_service::{ClientConfig, RemoteEmbedder};
use udas_embedding::{Embedder, FnvHashEmbedder};

/// Map an angle quadrant to a human-readable dimension description used
/// inside LLM prompts.
fn quadrant_brief(q: u8) -> &'static str {
    match q {
        0 => {
            "temporal — time-based retrieval: when events happened, chronological order, temporal causality, recency"
        }
        1 => {
            "semantic — meaning-based retrieval: concepts, themes, definitions, semantic relationships, analogies"
        }
        2 => {
            "entity — actor-based retrieval: who was involved, organisations, people, named entities, agents"
        }
        _ => {
            "cross-domain — conflict-based retrieval: contradictions, opposing views, edge cases, anomalies, paradoxes"
        }
    }
}

/// Extract the concatenated text from a `MessageResponse` content vector.
fn extract_text(blocks: &[ContentBlock]) -> String {
    // Prefer Text blocks (final output). DeepSeek V4 thinking-mode may
    // put content in Thinking blocks when reasoning_effort is set —
    // fall back to those if no Text blocks are present.
    let text: String = blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    if !text.trim().is_empty() {
        return text;
    }
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Thinking { thinking } => Some(thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip Markdown code fences (```json ... ```) that models sometimes wrap
/// around JSON outputs, returning the inner JSON text.
fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        // skip optional language tag on the first line
        let rest = rest.trim_start_matches(|c: char| c.is_alphanumeric());
        let rest = rest.trim_start_matches('\n');
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
        return rest.trim();
    }
    trimmed
}

// ─── LlmRestorer implementation ──────────────────────────────────────────

#[async_trait]
impl LlmRestorer for DeepSeekClient {
    async fn decompose(&self, problem: &str, angle: &Angle) -> anyhow::Result<Vec<String>> {
        let brief = quadrant_brief(angle.quadrant());

        let system = format!(
            "You are a retrieval strategist for the UDAS (Unitary Disk Active Search) system.\n\
             Your task: decompose a judgment problem into 2-5 retrieval sub-questions\n\
             optimised for the **{brief}** dimension.\n\n\
             Rules:\n\
             1. Each sub-question MUST target the {brief} dimension specifically.\n\
             2. Sub-questions should be answerable from memory, context, or reasoning.\n\
             3. Output ONLY a JSON array of strings. No commentary, no markdown.\n\
             4. Example shape: [\"sub-question 1\", \"sub-question 2\"]\n\
             5. If the problem is unclear, still produce your best 2 sub-questions."
        );

        let request = MessageRequest {
            model: self.model().to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: format!(
                        "Problem: {problem}\n\nGenerate 2-5 retrieval sub-questions for the {brief} dimension:"
                    ),
                    cache_control: None,
                }],
            }],
            max_tokens: 1024,
            system: Some(SystemPrompt::Text(system)),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: Some("off".into()),
            stream: Some(false),
            temperature: Some(0.3),
            top_p: None,
        };

        let response = self.create_message(request).await?;
        let raw = extract_text(&response.content);
        let cleaned = strip_code_fences(&raw);

        // Try strict JSON parse first, fall back to line splitting.
        match serde_json::from_str::<Vec<String>>(cleaned) {
            Ok(qs) if !qs.is_empty() => Ok(qs),
            _ => {
                let fallback: Vec<String> = cleaned
                    .lines()
                    .map(|l| {
                        l.trim().trim_start_matches(|c: char| {
                            c.is_numeric() || c == '.' || c == ')' || c == ' '
                        })
                    })
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                if fallback.is_empty() {
                    anyhow::bail!("decompose: LLM returned no parseable sub-questions: {raw}");
                }
                Ok(fallback)
            }
        }
    }

    async fn find_evidence(
        &self,
        angle: &Angle,
        key: &str,
        sub_questions: &[String],
    ) -> anyhow::Result<Vec<EvidenceItem>> {
        let brief = quadrant_brief(angle.quadrant());
        let angle_deg = angle.degrees;
        let qs = sub_questions
            .iter()
            .map(|q| format!("  - {q}"))
            .collect::<Vec<_>>()
            .join("\n");

        let system = format!(
            "You are an evidence retriever for the UDAS system.\n\
             Given retrieval sub-questions, search your context window and knowledge\n\
             for relevant evidence fragments.\n\n\
             Output ONLY a JSON array. Each element MUST have these fields:\n\
             - \"source\": origin of the evidence (string)\n\
             - \"content\": the evidence text (string)\n\
             - \"relevance_score\": relevance to the sub-questions, in [0.0, 1.0] (number)\n\n\
             Rules:\n\
             1. Target the **{brief}** dimension via the \"{key}\" retrieval strategy.\n\
             2. Return 1-5 evidence items. Quality over quantity.\n\
             3. If no relevant evidence exists, return an empty array [].\n\
             4. No markdown, no commentary — only the JSON array.\n\
             Example: [{{\"source\":\"memory\",\"content\":\"...\",\"relevance_score\":0.8}}]"
        );

        let request = MessageRequest {
            model: self.model().to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: format!(
                        "Angle: {angle_deg:.1}° ({brief})\nRetrieval strategy: {key}\nSub-questions:\n{qs}\n\nRetrieve evidence:"
                    ),
                    cache_control: None,
                }],
            }],
            max_tokens: 1024,
            system: Some(SystemPrompt::Text(system)),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: Some("off".into()),
            stream: Some(false),
            temperature: Some(0.2),
            top_p: None,
        };

        let response = self.create_message(request).await?;
        let raw = extract_text(&response.content);
        let cleaned = strip_code_fences(&raw);

        // Parse JSON array of objects, then map to EvidenceItem.
        #[derive(serde::Deserialize)]
        struct RawItem {
            source: String,
            content: String,
            relevance_score: f64,
        }

        match serde_json::from_str::<Vec<RawItem>>(cleaned) {
            Ok(items) => Ok(items
                .into_iter()
                .map(|r| EvidenceItem {
                    source: r.source,
                    content: r.content,
                    relevance_score: r.relevance_score.clamp(0.0, 1.0),
                    timestamp: Some(chrono::Utc::now()),
                })
                .collect()),
            Err(e) => {
                // If the model returned prose instead of JSON, treat the whole
                // text as a single low-confidence evidence item so the pipeline
                // does not hard-fail.
                if cleaned.trim().is_empty() {
                    return Ok(Vec::new());
                }
                tracing::warn!("find_evidence: JSON parse failed ({e}), using raw text fallback");
                Ok(vec![EvidenceItem {
                    source: "llm_raw".into(),
                    content: cleaned.to_string(),
                    relevance_score: 0.3,
                    timestamp: Some(chrono::Utc::now()),
                }])
            }
        }
    }

    async fn embed(&self, text: &str) -> anyhow::Result<Embedding> {
        let mode = *self.embedding_mode.lock().await;

        match mode {
            EmbeddingMode::Fnv => {
                // Fast path: FNV hash directly, skip remote service entirely.
                // No connection attempt, no timeout overhead.
                FnvHashEmbedder.embed(text).await
            }
            EmbeddingMode::BgeM3 => {
                // Semantic path: use remote BGE-M3 embedding service.
                // Check for a cached remote embedder connection first.
                {
                    let guard = self.remote_embedder.lock().await;
                    if let Some(ref remote) = *guard {
                        return remote.embed(text).await;
                    }
                }

                // No cached connection -- try to connect to the embedding service.
                let config = ClientConfig::default();
                match RemoteEmbedder::connect(config).await {
                    Ok(remote) => {
                        // Embed via the remote service, then cache the connection.
                        let result = remote.embed(text).await;
                        let mut guard = self.remote_embedder.lock().await;
                        *guard = Some(remote);
                        result
                    }
                    Err(e) => {
                        // Service not running -- fall back to FNV with warning.
                        // This allows the pipeline to continue, but the user
                        // should start the service for semantic accuracy.
                        tracing::warn!(
                            error = %e,
                            "embed: BGE-M3 mode active but embedding service unavailable. \
                             Falling back to FNV. Start service with: \
                             udas-cli embed-service start"
                        );
                        FnvHashEmbedder.embed(text).await
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // FNV embedding tests are now in the udas-embedding crate.
    // Only bridge-specific tests remain here.

    #[test]
    fn strip_code_fences_removes_json_fence() {
        assert_eq!(strip_code_fences("```json\n[1,2]\n```"), "[1,2]");
        assert_eq!(strip_code_fences("```\n[1,2]\n```"), "[1,2]");
        assert_eq!(strip_code_fences("[1,2]"), "[1,2]");
    }

    #[test]
    fn extract_text_concatenates_text_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "hello ".into(),
                cache_control: None,
            },
            ContentBlock::Text {
                text: "world".into(),
                cache_control: None,
            },
        ];
        assert_eq!(extract_text(&blocks), "hello world");
    }

    #[test]
    fn quadrant_brief_covers_all_quadrants() {
        assert!(quadrant_brief(0).contains("temporal"));
        assert!(quadrant_brief(1).contains("semantic"));
        assert!(quadrant_brief(2).contains("entity"));
        assert!(quadrant_brief(3).contains("cross-domain"));
    }
}
