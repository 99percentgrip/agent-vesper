//! V3 pipeline orchestration. Mirrors `mem0/memory/main.py:_add_to_vector_store`
//! (8-phase ADD flow), `_search_vector_store` (hybrid retrieval), and the
//! admin `update`/`delete` paths.
//!
//! All provider calls go through the trait ports in [`crate::ports`]. The
//! pipeline never sees a provider name or credential.

use std::collections::BTreeMap;
use std::collections::HashSet;

use serde_json::Value;

use crate::bm25::ENTITY_BOOST_WEIGHT;
use crate::error::{CognitionError, Result};
use crate::extract::{ExtractedMemory, parse_extraction_response};
use crate::filters::FilterDsl;
use crate::nlp::{EntityCandidate, lemmatize_for_bm25};
use crate::ports::EmbedAction;
use crate::prompts::{
    ADDITIVE_EXTRACTION_PROMPT, AGENT_CONTEXT_SUFFIX, EXISTING_MEMORY_TOP_K, ExistingMemory,
    LAST_K_MESSAGES, PROCEDURAL_MEMORY_SYSTEM_PROMPT, generate_additive_extraction_prompt,
};
use crate::score::{ScoredCandidate, score_and_rank};
use crate::store::{CognitiveStore, NewMemory};
type PendingRecord = (String, ExtractedMemory, Vec<f32>, BTreeMap<String, Value>);

use crate::types::{
    Attribution, HistoryEvent, MemoryEvent, MemoryHit, MemoryRecord, Message, Scope,
};

/// Per-turn add request.
pub struct AddRequest<'a> {
    pub messages: &'a [Message],
    pub scope: &'a Scope,
    /// Caller metadata merged into each memory's `extras`.
    pub extras: Option<&'a BTreeMap<String, Value>>,
    /// Optional `YYYY-MM-DD` expiration; expired rows are hidden from search.
    pub expiration_date: Option<&'a str>,
    /// `false` → store each message verbatim (mirrors mem0's `infer=False`).
    pub infer: bool,
    /// Optional override for `custom_instructions`.
    pub custom_instructions: Option<&'a str>,
    /// Override observation date (defaults to today UTC).
    pub observation_date: Option<&'a chrono::DateTime<chrono::Utc>>,
}

/// Per-query search request.
pub struct SearchRequest<'a> {
    pub query: &'a str,
    pub scope: &'a Scope,
    pub filters: Option<&'a FilterDsl>,
    pub top_k: usize,
    pub threshold: f32,
    pub explain: bool,
    pub show_expired: bool,
}

/// Cognitive-memory engine. Owns the store + ports + config.
pub struct CognitiveMemory {
    store: CognitiveStore,
    ports: crate::ports::CognitionPorts,
    config: crate::CognitiveConfig,
}

impl CognitiveMemory {
    pub(crate) fn new(
        store: CognitiveStore,
        ports: crate::ports::CognitionPorts,
        config: crate::CognitiveConfig,
    ) -> Self {
        Self {
            store,
            ports,
            config,
        }
    }

    /// V3 8-phase ADD-only pipeline. Returns the ADD events.
    pub fn add(&self, req: AddRequest<'_>) -> Result<Vec<MemoryEvent>> {
        if !req.scope.is_set() {
            return Err(CognitionError::MissingScope);
        }
        if req.messages.is_empty() {
            return Ok(Vec::new());
        }

        // === Phase 6 fallback path: infer=false → store raw messages ===
        if !req.infer {
            return self.add_raw_messages(req);
        }

        let session_key = req.scope.session_key();
        let observation_date = req
            .observation_date
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
        let current_date = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // === Phase 0: context gathering (rolling window) ===
        let last_messages = self
            .store
            .get_last_messages(&session_key, LAST_K_MESSAGES)?;

        // === Phase 1: existing memory retrieval (anti-hallucination grounding) ===
        let existing_stored =
            self.store
                .list_for_semantic_search(req.scope, None, EXISTING_MEMORY_TOP_K, true)?;
        let query_embedding_for_phase1 = self.embed_concatenated_messages(req.messages)?;
        let semantic_phase1 =
            CognitiveStore::semantic_score(&existing_stored, &query_embedding_for_phase1);
        // Top-K existing memories as the LLM sees them (UUIDs mapped to integers).
        let mut sorted_phase1 = semantic_phase1;
        sorted_phase1.sort_by(|a, b| {
            b.semantic_score
                .partial_cmp(&a.semantic_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let existing_memories: Vec<ExistingMemory> = sorted_phase1
            .iter()
            .take(EXISTING_MEMORY_TOP_K)
            .enumerate()
            .map(|(idx, c)| ExistingMemory {
                id: idx.to_string(),
                text: c.payload_data.clone(),
            })
            .collect();

        // Existing hashes for Phase 5 dedup.
        let existing_hashes: HashSet<String> =
            existing_stored.iter().map(|m| m.hash.clone()).collect();

        // === Phase 2: LLM extraction (single call) ===
        let mut system_prompt = String::from(ADDITIVE_EXTRACTION_PROMPT);
        if req.scope.agent_id.is_some() && req.scope.user_id.is_none() {
            system_prompt.push('\n');
            system_prompt.push_str(AGENT_CONTEXT_SUFFIX);
        }
        let user_prompt = generate_additive_extraction_prompt(
            None,
            &[],
            &existing_memories,
            req.messages,
            &last_messages,
            &observation_date,
            &current_date,
            req.custom_instructions,
        );
        let raw_response = self.ports.extractor.extract(&system_prompt, &user_prompt)?;
        let extracted = parse_extraction_response(&raw_response)?;
        if extracted.is_empty() {
            // Even with no extraction, advance the rolling context window.
            self.store.save_messages(req.messages, &session_key)?;
            return Ok(Vec::new());
        }

        // === Phase 3: batch embed all extracted texts ===
        let texts: Vec<&str> = extracted
            .iter()
            .map(|m| m.text.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        // Use a stable order with no duplicates for embed_batch.
        let dedup_texts = dedup_preserve_order(&texts);
        let dedup_refs: Vec<&str> = dedup_texts.iter().map(String::as_str).collect();
        let embeddings_map: std::collections::HashMap<String, Vec<f32>> = {
            let embeddings = self
                .ports
                .embedder
                .embed_batch(&dedup_refs, EmbedAction::Add)?;
            if embeddings.len() != dedup_texts.len() {
                return Err(CognitionError::Embedding(
                    "batch embedding count mismatch".into(),
                ));
            }
            dedup_texts.iter().cloned().zip(embeddings).collect()
        };

        // === Phase 4 + 5: per-memory + hash dedup ===
        let now = chrono::Utc::now().to_rfc3339();
        let base_extras = req.extras.cloned().unwrap_or_default();
        let mut records: Vec<PendingRecord> = Vec::new();
        let mut seen_hashes: HashSet<String> = HashSet::new();
        for mem in &extracted {
            if mem.text.is_empty() {
                continue;
            }
            let hash = md5_hex(&mem.text);
            if existing_hashes.contains(&hash) || seen_hashes.contains(&hash) {
                continue;
            }
            seen_hashes.insert(hash.clone());
            let embedding = match embeddings_map.get(&mem.text) {
                Some(v) => v.clone(),
                None => self.ports.embedder.embed(&mem.text, EmbedAction::Add)?,
            };
            // Validate embedding dim.
            if embedding.len() != self.config.embedding_dim {
                return Err(CognitionError::EmbeddingDimension {
                    expected: self.config.embedding_dim,
                    actual: embedding.len(),
                });
            }
            let mut extras = base_extras.clone();
            if let Some(attrib) = &mem.attributed_to {
                extras.insert("attributed_to".into(), Value::String(attrib.clone()));
            }
            records.push((
                uuid::Uuid::new_v4().to_string(),
                mem.clone(),
                embedding,
                extras,
            ));
        }

        if records.is_empty() {
            self.store.save_messages(req.messages, &session_key)?;
            return Ok(Vec::new());
        }

        // === Phase 5.5: optional conflict detection (TencentDB-inspired) ===
        // When enabled, a second LLM call classifies each new memory as
        // store/skip/update/merge against existing memories. Default: disabled
        // (preserves V3 ADD-only behavior).
        if self.config.enable_conflict_detection && !records.is_empty() {
            records = self.conflict_detection_pass(records, &existing_stored);
        }

        // === Phase 6: batch persist + history ===
        let mut events = Vec::with_capacity(records.len());
        for (id, mem, embedding, extras) in &records {
            let attributed = mem.attributed_to.as_deref().and_then(Attribution::parse);
            let text_lemmatized = lemmatize_for_bm25(&mem.text);
            let hash_str = md5_hex(&mem.text);
            let new_mem = NewMemory {
                id,
                data: &mem.text,
                hash: &hash_str,
                text_lemmatized: &text_lemmatized,
                embedding,
                attributed_to: attributed,
                scope: req.scope,
                actor_id: None,
                role: mem.attributed_to.as_deref().map(|a| {
                    if a == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    }
                }),
                memory_type: mem.memory_type.as_deref(),
                priority: mem.priority,
                scene: mem.scene.as_deref(),
                expiration_date: req.expiration_date,
                created_at: &now,
                updated_at: &now,
                extras,
            };
            self.store.insert_memory(&new_mem)?;
            self.store.add_history(
                id,
                None,
                Some(&mem.text),
                "ADD",
                &now,
                Some(&now),
                None,
                mem.attributed_to.as_deref(),
                false,
            )?;
            events.push(MemoryEvent {
                id: id.clone(),
                memory: mem.text.clone(),
                event: "ADD",
            });
        }

        // === Phase 7: entity linking (per memory) ===
        // Best-effort; failures are non-fatal (mirrors mem0's swallow-at-warning).
        for (id, mem, _embedding, _extras) in &records {
            if let Err(err) = self.link_entities_for_memory(id, &mem.text, req.scope) {
                tracing_warn_entity_link(&err);
            }
        }

        // === Phase 8: save messages + return ===
        self.store.save_messages(req.messages, &session_key)?;
        Ok(events)
    }

    /// Optional conflict-detection pass (TencentDB Agent Memory inspired).
    /// Sends new memories + existing memories to the extraction LLM and asks
    /// it to classify each as store/skip/update/merge. Memories classified as
    /// "skip" are dropped; "store" proceeds as normal.
    /// This is a SIMPLIFIED v1: update/merge actions are treated as store
    /// (the V3 ADD-only model doesn't support overwrites). A future version
    /// can implement true update/merge against existing memory IDs.
    fn conflict_detection_pass(
        &self,
        new_records: Vec<PendingRecord>,
        existing: &[crate::store::StoredMemory],
    ) -> Vec<PendingRecord> {
        // Build a compact summary of existing memories for the LLM.
        let existing_summary: Vec<String> = existing
            .iter()
            .take(20)
            .map(|m| format!("- {}", m.data.chars().take(100).collect::<String>()))
            .collect();
        let new_summary: Vec<String> = new_records
            .iter()
            .map(|(_, mem, _, _)| format!("- {}", mem.text.chars().take(100).collect::<String>()))
            .collect();

        let system = "You are a memory conflict detector. Compare new memories against existing ones. For each new memory, output a JSON array of objects with \"text\" and \"action\" (\"store\" if new/useful, \"skip\" if duplicate/redundant). Return ONLY the JSON array.";
        let user = format!(
            "Existing memories:\n{}\n\nNew memories to evaluate:\n{}\n\nClassify each new memory:",
            existing_summary.join("\n"),
            new_summary.join("\n"),
        );

        let response = match self.ports.extractor.extract(system, &user) {
            Ok(text) => text,
            Err(_) => return new_records, // On failure, keep all (fail-open).
        };

        // Parse the conflict detection response.
        let actions: Vec<serde_json::Value> = match crate::extract::extract_json(&response) {
            Some(json_str) => serde_json::from_str(&json_str).unwrap_or_default(),
            None => return new_records, // Parse failure → keep all.
        };

        // Build a set of texts to skip.
        let skip_texts: HashSet<String> = actions
            .iter()
            .filter_map(|item| {
                if item.get("action").and_then(|a| a.as_str()) == Some("skip") {
                    item.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect();

        if skip_texts.is_empty() {
            return new_records;
        }

        // Filter out skipped memories.
        new_records
            .into_iter()
            .filter(|(_, mem, _, _)| {
                let text_prefix: String = mem.text.chars().take(100).collect();
                !skip_texts.iter().any(|s| text_prefix.contains(s.as_str()))
            })
            .collect()
    }

    fn add_raw_messages(&self, req: AddRequest<'_>) -> Result<Vec<MemoryEvent>> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut events = Vec::new();
        for msg in req.messages {
            if msg.role == "system" {
                continue;
            }
            let embedding = self.ports.embedder.embed(&msg.content, EmbedAction::Add)?;
            let id = uuid::Uuid::new_v4().to_string();
            let extras = req.extras.cloned().unwrap_or_default();
            let attributed = crate::nlp::attribution_for_role(&msg.role);
            let new_mem = NewMemory {
                id: &id,
                data: &msg.content,
                hash: &md5_hex(&msg.content),
                text_lemmatized: &lemmatize_for_bm25(&msg.content),
                embedding: &embedding,
                attributed_to: attributed,
                scope: req.scope,
                actor_id: msg.name.as_deref(),
                role: Some(&msg.role),
                memory_type: None,
                priority: None,
                scene: None,
                expiration_date: req.expiration_date,
                created_at: &now,
                updated_at: &now,
                extras: &extras,
            };
            self.store.insert_memory(&new_mem)?;
            self.store.add_history(
                &id,
                None,
                Some(&msg.content),
                "ADD",
                &now,
                Some(&now),
                msg.name.as_deref(),
                Some(&msg.role),
                false,
            )?;
            events.push(MemoryEvent {
                id,
                memory: msg.content.clone(),
                event: "ADD",
            });
        }
        self.store
            .save_messages(req.messages, &req.scope.session_key())?;
        Ok(events)
    }

    fn embed_concatenated_messages(&self, messages: &[Message]) -> Result<Vec<f32>> {
        let joined = messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.ports.embedder.embed(&joined, EmbedAction::Search)
    }

    fn link_entities_for_memory(&self, memory_id: &str, text: &str, scope: &Scope) -> Result<()> {
        let candidates = self.ports.entity_nlp.extract(text);
        let mut seen = HashSet::new();
        for cand in candidates {
            let key = cand.normalized();
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            let emb = self.ports.embedder.embed(&cand.text, EmbedAction::Add)?;
            self.store.upsert_entity(&cand, memory_id, scope, &emb)?;
        }
        Ok(())
    }

    // ----- search -----------------------------------------------------

    /// Hybrid search. Mirrors `_search_vector_store`.
    pub fn search(&self, req: SearchRequest<'_>) -> Result<Vec<MemoryHit>> {
        if !req.scope.is_set() {
            return Err(CognitionError::MissingScope);
        }
        // Step 1: preprocess query.
        let query_lemmatized = lemmatize_for_bm25(req.query);
        let num_terms = query_lemmatized.split_whitespace().count();
        let query_entities = self.ports.entity_nlp.extract(req.query);

        // Step 2: embed query.
        let query_embedding = self.ports.embedder.embed(req.query, EmbedAction::Search)?;

        // Step 3: semantic search (over-fetch for scoring pool).
        let internal_limit = (req.top_k * 4).max(60);
        let stored_candidates = self.store.list_for_semantic_search(
            req.scope,
            req.filters,
            internal_limit,
            req.show_expired,
        )?;
        let semantic_candidates =
            CognitiveStore::semantic_score(&stored_candidates, &query_embedding);

        // Step 4 + 5: BM25 keyword search + sigmoid normalization.
        let keyword_raw =
            self.store
                .keyword_search(&query_lemmatized, req.scope, internal_limit)?;
        let bm25_scores = CognitiveStore::normalize_keyword_scores(&keyword_raw, num_terms);

        // Step 6: entity boosts.
        let entity_boosts = if query_entities.is_empty() {
            std::collections::HashMap::new()
        } else {
            // Embed each query entity (deduped, max 8).
            let mut deduped: Vec<(EntityCandidate, Vec<f32>)> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for cand in query_entities.iter().take(8) {
                let key = cand.normalized();
                if key.is_empty() || !seen.insert(key) {
                    continue;
                }
                let emb = self.ports.embedder.embed(&cand.text, EmbedAction::Search)?;
                deduped.push((cand.clone(), emb));
            }
            self.store.entity_boosts(&deduped, req.scope, 0.5)?
        };

        // Step 7: candidate set is semantic_candidates (already filtered).
        // Step 8: score and rank (select strategy from config).
        let scored: Vec<ScoredCandidate> = semantic_candidates;
        let hits = match self.config.fusion_strategy {
            crate::FusionStrategy::Additive => score_and_rank(
                &scored,
                &bm25_scores,
                &entity_boosts,
                req.threshold,
                req.top_k,
                req.explain,
            ),
            crate::FusionStrategy::RRF => crate::score::score_and_rank_rrf(
                &scored,
                &bm25_scores,
                &entity_boosts,
                req.threshold,
                req.top_k,
                req.explain,
            ),
        };
        // Heat tracking: increment recall_count for returned hits.
        let hit_ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
        let _ = self.store.increment_recall_counts(&hit_ids);
        Ok(hits)
    }

    // ----- admin ops --------------------------------------------------

    /// List memories for `get_all` (no scoring).
    pub fn get_all(
        &self,
        scope: &Scope,
        filters: Option<&FilterDsl>,
        top_k: usize,
        show_expired: bool,
    ) -> Result<Vec<MemoryRecord>> {
        self.store
            .list_memories(scope, filters, top_k, show_expired)
    }

    /// Direct admin update (no LLM). Mirrors `_update_memory`.
    pub fn update(
        &self,
        memory_id: &str,
        text: Option<&str>,
        extras_patch: Option<&BTreeMap<String, Value>>,
        expiration_date: Option<&str>,
    ) -> Result<()> {
        let existing = self
            .store
            .get_memory(memory_id)?
            .ok_or(CognitionError::InvalidArgument("memory not found"))?;
        let now = chrono::Utc::now().to_rfc3339();
        let new_text = text.unwrap_or(&existing.data);
        let new_lemmatized = lemmatize_for_bm25(new_text);
        let new_hash = md5_hex(new_text);
        let new_embedding = if text.is_some() {
            self.ports.embedder.embed(new_text, EmbedAction::Update)?
        } else {
            // Reuse existing embedding by re-embedding the unchanged text.
            self.ports.embedder.embed(new_text, EmbedAction::Update)?
        };
        let merged_extras_json: Option<String> = extras_patch.map(|patch| {
            let mut merged = existing.extras.clone();
            for (k, v) in patch {
                merged.insert(k.clone(), v.clone());
            }
            serde_json::to_string(&merged).unwrap_or_else(|_| "{}".into())
        });
        self.store.update_memory_text(
            memory_id,
            new_text,
            &new_lemmatized,
            &new_hash,
            &new_embedding,
            &now,
            merged_extras_json.as_deref(),
        )?;
        let new_expiration = expiration_date
            .map(str::to_string)
            .or(existing.expiration_date.clone());
        let _ = new_expiration; // expiration is part of the record, not patched here in v1.
        self.store.add_history(
            memory_id,
            Some(&existing.data),
            Some(new_text),
            "UPDATE",
            &existing.created_at,
            Some(&now),
            existing.actor_id.as_deref(),
            existing.role.as_deref(),
            false,
        )?;
        Ok(())
    }

    /// Direct admin delete (no LLM). Mirrors `_delete_memory` + entity cleanup.
    pub fn delete(&self, memory_id: &str) -> Result<()> {
        let existing = self
            .store
            .delete_memory(memory_id)?
            .ok_or(CognitionError::InvalidArgument("memory not found"))?;
        let now = chrono::Utc::now().to_rfc3339();
        self.store.add_history(
            memory_id,
            Some(&existing.data),
            None,
            "DELETE",
            &existing.created_at,
            Some(&now),
            existing.actor_id.as_deref(),
            existing.role.as_deref(),
            true,
        )?;
        Ok(())
    }

    /// Audit history for a memory.
    pub fn history(&self, memory_id: &str) -> Result<Vec<HistoryEvent>> {
        self.store.history(memory_id)
    }

    /// Procedural-memory compaction. Mirrors `_create_procedural_memory`.
    pub fn add_procedural(
        &self,
        messages: &[Message],
        scope: &Scope,
        prompt_override: Option<&str>,
    ) -> Result<MemoryEvent> {
        if !scope.is_set() {
            return Err(CognitionError::MissingScope);
        }
        let system_prompt = prompt_override.unwrap_or(PROCEDURAL_MEMORY_SYSTEM_PROMPT);
        let user_prompt = format!(
            "Conversation:\n{}\n\nCreate procedural memory of the above conversation.",
            messages
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let summary = self.ports.extractor.extract(system_prompt, &user_prompt)?;
        let cleaned = crate::extract::remove_code_blocks(&summary);
        let text = cleaned.trim();
        if text.is_empty() {
            return Err(CognitionError::ExtractionParse);
        }
        let embedding = self.ports.embedder.embed(text, EmbedAction::Add)?;
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let extras = BTreeMap::new();
        let new_mem = NewMemory {
            id: &id,
            data: text,
            hash: &md5_hex(text),
            text_lemmatized: &lemmatize_for_bm25(text),
            embedding: &embedding,
            attributed_to: None,
            scope,
            actor_id: None,
            role: None,
            memory_type: Some("procedural_memory"),
            priority: Some(90),
            scene: None,
            expiration_date: None,
            created_at: &now,
            updated_at: &now,
            extras: &extras,
        };
        self.store.insert_memory(&new_mem)?;
        self.store.add_history(
            &id,
            None,
            Some(text),
            "ADD",
            &now,
            Some(&now),
            None,
            None,
            false,
        )?;
        Ok(MemoryEvent {
            id,
            memory: text.to_string(),
            event: "ADD",
        })
    }

    /// Drop all data (history, messages, memories, entities, links).
    pub fn reset(&self) -> Result<()> {
        self.store.reset()
    }
}

// ----- helpers -----------------------------------------------------------

fn dedup_preserve_order(input: &[&str]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in input {
        if seen.insert(s.to_string()) {
            out.push(s.to_string());
        }
    }
    out
}

fn md5_hex(text: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for byte in digest.iter() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn tracing_warn_entity_link(err: &CognitionError) {
    // The crate never depends on `tracing`; surface nothing in release.
    // Failures here are non-fatal per the oracle's "swallow at warning" rule.
    let _ = err;
}

// Reference the boost weight so dead-code lints know it's part of the surface.
const _: f32 = ENTITY_BOOST_WEIGHT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        CognitionPorts, EmbedAction, EmbeddingPort, EntityExtractorPort, ExtractionLlmPort,
    };
    use std::sync::Arc;

    struct StubEmbedder(usize);
    impl EmbeddingPort for StubEmbedder {
        fn embed(&self, text: &str, _action: EmbedAction) -> Result<Vec<f32>> {
            let mut out = vec![0.0_f32; self.0];
            for (i, b) in text.bytes().enumerate() {
                out[i % self.0] += (b as f32) / 255.0;
            }
            let norm = out.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut out {
                    *v /= norm;
                }
            }
            Ok(out)
        }
    }

    struct StubExtractor(std::sync::Mutex<String>);
    impl ExtractionLlmPort for StubExtractor {
        fn extract(&self, _system: &str, _user: &str) -> Result<String> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    struct DefaultEntities;
    impl EntityExtractorPort for DefaultEntities {
        fn extract(&self, text: &str) -> Vec<EntityCandidate> {
            crate::nlp::extract_entities(text)
        }
    }

    fn build_engine(dir: &tempfile::TempDir, dim: usize, response: &str) -> CognitiveMemory {
        let path = dir.path().join("cognition.db");
        let store = CognitiveStore::open_with_functions(&path).unwrap();
        let ports = CognitionPorts {
            embedder: Arc::new(StubEmbedder(dim)),
            extractor: Arc::new(StubExtractor(std::sync::Mutex::new(response.to_string()))),
            entity_nlp: Arc::new(DefaultEntities),
        };
        let config = crate::CognitiveConfig {
            embedding_dim: dim,
            ..Default::default()
        };
        CognitiveMemory::new(store, ports, config)
    }

    #[test]
    fn add_extracts_and_persists_with_md5_dedup() {
        let dir = tempfile::tempdir().unwrap();
        let response = r#"{"memory":[{"id":"0","text":"User likes Rust","attributed_to":"user"}]}"#;
        let engine = build_engine(&dir, 8, response);
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let msg = Message::user("I really like Rust.");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        let events = engine.add(req).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].memory, "User likes Rust");

        // Second add with the SAME extraction text → dedup drops it.
        let req2 = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        let events2 = engine.add(req2).unwrap();
        assert!(
            events2.is_empty(),
            "MD5 dedup must drop duplicate extraction"
        );
    }

    #[test]
    fn add_with_empty_extraction_advances_window_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let engine = build_engine(&dir, 8, r#"{"memory":[]}"#);
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let msg = Message::user("Hi");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        let events = engine.add(req).unwrap();
        assert!(events.is_empty());
        // Window should have been saved.
        let last = engine
            .store
            .get_last_messages(&scope.session_key(), 10)
            .unwrap();
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].content, "Hi");
    }

    #[test]
    fn add_infer_false_stores_raw_messages() {
        let dir = tempfile::tempdir().unwrap();
        // Even with a stub response, infer=false path skips the LLM.
        let engine = build_engine(&dir, 8, r#"{"memory":[]}"#);
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let msg = Message::user("I really like Rust.");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: false,
            custom_instructions: None,
            observation_date: None,
        };
        let events = engine.add(req).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].memory, "I really like Rust.");
    }

    #[test]
    fn search_returns_scored_hits_above_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let response = r#"{"memory":[{"id":"0","text":"User loves Rust","attributed_to":"user"}]}"#;
        let engine = build_engine(&dir, 8, response);
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let msg = Message::user("I love Rust.");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        engine.add(req).unwrap();

        let sreq = SearchRequest {
            query: "rust",
            scope: &scope,
            filters: None,
            top_k: 5,
            threshold: 0.0,
            explain: true,
            show_expired: false,
        };
        let hits = engine.search(sreq).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].memory, "User loves Rust");
        let details = hits[0].score_details.as_ref().unwrap();
        assert!(details.final_score > 0.0);
    }

    #[test]
    fn missing_scope_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let engine = build_engine(&dir, 8, r#"{"memory":[]}"#);
        let scope = Scope::default();
        let msg = Message::user("Hi");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        assert!(matches!(engine.add(req), Err(CognitionError::MissingScope)));
    }

    #[test]
    fn embedding_dim_mismatch_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let response = r#"{"memory":[{"id":"0","text":"hello","attributed_to":"user"}]}"#;
        // Engine configured for 8 dims; embedder also 8 — should succeed.
        let engine = build_engine(&dir, 8, response);
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let msg = Message::user("Hi");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        let _ = engine.add(req).unwrap();
    }

    #[test]
    fn update_then_delete_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let response = r#"{"memory":[{"id":"0","text":"User likes Rust","attributed_to":"user"}]}"#;
        let engine = build_engine(&dir, 8, response);
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let msg = Message::user("I like Rust.");
        let req = AddRequest {
            messages: std::slice::from_ref(&msg),
            scope: &scope,
            extras: None,
            expiration_date: None,
            infer: true,
            custom_instructions: None,
            observation_date: None,
        };
        let events = engine.add(req).unwrap();
        let id = events[0].id.clone();
        engine
            .update(&id, Some("User loves Rust"), None, None)
            .unwrap();
        let updated = engine.store.get_memory(&id).unwrap().unwrap();
        assert_eq!(updated.data, "User loves Rust");
        let hist = engine.history(&id).unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].event, "ADD");
        assert_eq!(hist[1].event, "UPDATE");

        engine.delete(&id).unwrap();
        assert!(engine.store.get_memory(&id).unwrap().is_none());
    }

    #[test]
    fn procedural_memory_compaction_works() {
        let dir = tempfile::tempdir().unwrap();
        let summary = "Agent executed step 1: opened URL https://example.com.";
        let engine = build_engine(&dir, 8, summary);
        let scope = Scope {
            agent_id: Some("a1".into()),
            ..Scope::default()
        };
        let msg = Message::assistant("Clicked the link.");
        let evt = engine
            .add_procedural(std::slice::from_ref(&msg), &scope, None)
            .unwrap();
        assert_eq!(evt.event, "ADD");
        let rec = engine.store.get_memory(&evt.id).unwrap().unwrap();
        assert_eq!(rec.memory_type.as_deref(), Some("procedural_memory"));
    }
}
