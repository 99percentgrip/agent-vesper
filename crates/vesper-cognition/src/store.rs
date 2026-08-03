//! SQLite-backed cognitive-memory store. Owns the connection, schema
//! bootstrap, FTS5 BM25 index, and low-level CRUD primitives. Pure data
//! access — no LLM/embedding orchestration (that lives in `pipeline.rs`).
//!
//! Schema is the relational + FTS5 layout ratified by ADR 0015 and described
//! in the foundation blueprint. The memory content lives in the `memories`
//! table (the source of truth); `memories_fts` is a derived BM25 index kept
//! in sync by triggers; `entities` + `entity_memory_links` form the entity
//! graph; `history` is the audit log; `messages` is the rolling 10-row
//! session window.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::bm25::{get_bm25_params, normalize_bm25};
use crate::error::{CognitionError, Result};
use crate::filters::FilterDsl;
use crate::nlp::EntityCandidate;
use crate::score::{ScoredCandidate, cosine};
use crate::types::{Attribution, HistoryEvent, MemoryRecord, Message, Scope};

/// Maximum number of messages retained per session scope.
pub const ROLLING_MESSAGE_LIMIT: usize = 10;

/// One row of `memories` plus its decoded embedding. Returned by
/// `list_for_semantic_search` for cosine scoring.
#[allow(dead_code)]
pub(crate) struct StoredMemory {
    pub id: String,
    pub data: String,
    pub hash: String,
    pub text_lemmatized: String,
    pub embedding: Vec<f32>,
    pub attributed_to: Option<String>,
    pub actor_id: Option<String>,
    pub role: Option<String>,
    pub memory_type: Option<String>,
    pub expiration_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub extras: std::collections::BTreeMap<String, Value>,
    pub scope: Scope,
}

/// One row of `entities` plus its linked memory IDs. Reserved for future
/// direct-entity queries; currently the entity-boost path builds its own
/// structures inline. Kept as documentation of the entity-row shape.
#[allow(dead_code)]
pub(crate) struct StoredEntity {
    pub id: String,
    pub data: String,
    pub data_normalized: String,
    pub entity_type: String,
    pub embedding: Vec<f32>,
    pub linked_memory_ids: Vec<String>,
    pub scope: Scope,
}

/// SQLite-backed cognitive-memory store. `Send + Sync` via interior mutex.
pub struct CognitiveStore {
    conn: Mutex<Connection>,
}

impl CognitiveStore {
    /// Open (or create) the store at the given path. The parent directory
    /// MUST exist; this method never creates directories.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(CognitionError::InvalidRoot);
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            return Err(CognitionError::InvalidRoot);
        }
        let conn = Connection::open(path)?;
        // WAL + busy timeout, mirroring the sqlite-fts5-spike verdict.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Self::bootstrap(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn bootstrap(conn: &Connection) -> Result<()> {
        // FTS5 availability check (per sqlite-fts5-spike verdict).
        let fts5_ok: Option<i64> = conn
            .query_row(
                "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if fts5_ok.unwrap_or(0) == 0 {
            return Err(CognitionError::Storage(
                "FTS5 is not enabled in the bundled SQLite build".to_string(),
            ));
        }

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS history (
                id           TEXT PRIMARY KEY,
                memory_id    TEXT,
                old_memory   TEXT,
                new_memory   TEXT,
                event        TEXT,
                created_at   TEXT,
                updated_at   TEXT,
                is_deleted   INTEGER NOT NULL DEFAULT 0,
                actor_id     TEXT,
                role         TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_history_memory_id ON history(memory_id);
            CREATE INDEX IF NOT EXISTS idx_history_created   ON history(created_at);

            CREATE TABLE IF NOT EXISTS messages (
                id            TEXT PRIMARY KEY,
                session_scope TEXT NOT NULL,
                role          TEXT,
                content       TEXT,
                name          TEXT,
                created_at    TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_messages_scope_time
                ON messages(session_scope, created_at DESC);

            CREATE TABLE IF NOT EXISTS memories (
                id              TEXT PRIMARY KEY,
                data            TEXT NOT NULL,
                hash            TEXT NOT NULL,
                text_lemmatized TEXT NOT NULL,
                embedding       BLOB NOT NULL,
                attributed_to   TEXT,
                user_id         TEXT,
                agent_id        TEXT,
                run_id          TEXT,
                actor_id        TEXT,
                role            TEXT,
                memory_type     TEXT,
                expiration_date TEXT,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                extras          TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_memories_hash  ON memories(hash);
            CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(user_id, agent_id, run_id);

            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                text_lemmatized,
                memory_id UNINDEXED,
                tokenize = 'unicode61 remove_diacritics 2'
            );

            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, text_lemmatized, memory_id)
                VALUES (new.rowid, new.text_lemmatized, new.id);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                DELETE FROM memories_fts WHERE memory_id = old.id;
            END;
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                DELETE FROM memories_fts WHERE memory_id = old.id;
                INSERT INTO memories_fts(rowid, text_lemmatized, memory_id)
                VALUES (new.rowid, new.text_lemmatized, new.id);
            END;

            CREATE TABLE IF NOT EXISTS entities (
                id              TEXT PRIMARY KEY,
                data            TEXT NOT NULL,
                data_normalized TEXT NOT NULL,
                entity_type     TEXT NOT NULL,
                embedding       BLOB NOT NULL,
                user_id         TEXT,
                agent_id        TEXT,
                run_id          TEXT,
                UNIQUE(data_normalized, user_id, agent_id, run_id)
            );
            CREATE INDEX IF NOT EXISTS idx_entities_scope ON entities(user_id, agent_id, run_id);

            CREATE TABLE IF NOT EXISTS entity_memory_links (
                entity_id   TEXT NOT NULL,
                memory_id   TEXT NOT NULL,
                PRIMARY KEY (entity_id, memory_id)
            );
            CREATE INDEX IF NOT EXISTS idx_eml_entity ON entity_memory_links(entity_id);
            CREATE INDEX IF NOT EXISTS idx_eml_memory ON entity_memory_links(memory_id);
            ",
        )?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .expect("cognition store mutex poisoned; panicking per Rust convention")
    }

    // ----- message rolling window --------------------------------------

    /// Save messages, then prune to the most recent `ROLLING_MESSAGE_LIMIT`
    /// for this scope. Mirrors `SQLiteManager.save_messages`.
    pub fn save_messages(&self, messages: &[Message], session_scope: &str) -> Result<()> {
        let conn = self.lock();
        let now = now_iso();
        for msg in messages {
            let id = uuid_str();
            conn.execute(
                "INSERT INTO messages (id, session_scope, role, content, name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, session_scope, msg.role, msg.content, msg.name, now],
            )?;
        }
        // Prune to last N.
        conn.execute(
            "DELETE FROM messages WHERE session_scope = ?1 AND id NOT IN (
                SELECT id FROM (
                    SELECT id FROM messages WHERE session_scope = ?1
                    ORDER BY created_at DESC LIMIT ?2
                )
            )",
            params![session_scope, ROLLING_MESSAGE_LIMIT as i64],
        )?;
        Ok(())
    }

    /// Return the last `limit` messages in chronological order. Mirrors
    /// `SQLiteManager.get_last_messages`.
    pub fn get_last_messages(&self, session_scope: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT role, content, name FROM (
                SELECT role, content, name, created_at FROM messages
                WHERE session_scope = ?1
                ORDER BY created_at DESC LIMIT ?2
            ) ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_scope, limit as i64], |row| {
            Ok(Message {
                role: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                content: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                name: row.get::<_, Option<String>>(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ----- memory CRUD -------------------------------------------------

    /// Insert a memory row. Caller supplies the lemmatized text and embedding.
    pub(crate) fn insert_memory(&self, record: &NewMemory<'_>) -> Result<()> {
        let conn = self.lock();
        let embedding_blob = embed_to_blob(record.embedding);
        let extras_json = serde_json::to_string(&record.extras).unwrap_or_else(|_| "{}".into());
        let id = record.id.to_string();
        conn.execute(
            "INSERT INTO memories (
                id, data, hash, text_lemmatized, embedding, attributed_to,
                user_id, agent_id, run_id, actor_id, role, memory_type,
                expiration_date, created_at, updated_at, extras
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                id,
                record.data,
                record.hash,
                record.text_lemmatized,
                embedding_blob,
                record.attributed_to.map(|a| a.as_str()),
                record.scope.user_id,
                record.scope.agent_id,
                record.scope.run_id,
                record.actor_id,
                record.role,
                record.memory_type,
                record.expiration_date,
                record.created_at,
                record.updated_at,
                extras_json,
            ],
        )?;
        Ok(())
    }

    /// Return true if a memory with this hash already exists in scope.
    #[allow(dead_code)]
    pub(crate) fn hash_exists(&self, hash: &str, scope: &Scope) -> Result<bool> {
        let conn = self.lock();
        let mut bindings: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(hash.to_string())];
        let mut clauses = vec!["hash = ?".to_string()];
        if let Some(v) = &scope.user_id {
            clauses.push("user_id = ?".into());
            bindings.push(Box::new(v.clone()));
        }
        if let Some(v) = &scope.agent_id {
            clauses.push("agent_id = ?".into());
            bindings.push(Box::new(v.clone()));
        }
        if let Some(v) = &scope.run_id {
            clauses.push("run_id = ?".into());
            bindings.push(Box::new(v.clone()));
        }
        let sql = format!(
            "SELECT 1 FROM memories WHERE {} LIMIT 1",
            clauses.join(" AND ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let exists: Option<i64> = stmt
            .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
                row.get(0)
            })?
            .next()
            .transpose()?;
        Ok(exists.unwrap_or(0) > 0)
    }

    /// Bulk-fetch candidate memory rows for semantic scoring. Filters by
    /// scope; applies the optional `FilterDsl` against `extras` in Rust.
    /// Skips expired rows unless `show_expired`.
    pub(crate) fn list_for_semantic_search(
        &self,
        scope: &Scope,
        filter: Option<&FilterDsl>,
        limit: usize,
        show_expired: bool,
    ) -> Result<Vec<StoredMemory>> {
        let conn = self.lock();
        let (where_clause, bindings) = scope_where(scope, show_expired);
        let sql = format!(
            "SELECT id, data, hash, text_lemmatized, embedding, attributed_to,
                    user_id, agent_id, run_id, actor_id, role, memory_type,
                    expiration_date, created_at, updated_at, extras
             FROM memories {where_clause} ORDER BY created_at DESC LIMIT ?"
        );
        let mut all_bindings: Vec<Box<dyn rusqlite::ToSql>> = bindings;
        all_bindings.push(Box::new(limit.to_string()));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(all_bindings.iter()),
            row_to_memory,
        )?;
        let mut out = Vec::new();
        for row in rows {
            let stored = row?;
            if let Some(f) = filter
                && !f.matches(&stored.extras)
            {
                continue;
            }
            out.push(stored);
        }
        Ok(out)
    }

    /// FTS5 BM25 keyword search over lemmatized text. Returns
    /// `(memory_id, raw_bm25_score)` pairs. Mirrors the oracle's
    /// `vector_store.keyword_search`. The raw score is FTS5's `bm25()` output
    /// (negative, lower = better) — caller negates before normalization.
    /// Scope filtering happens in Rust to avoid a custom SQL function.
    pub(crate) fn keyword_search(
        &self,
        lemmatized_query: &str,
        scope: &Scope,
        top_k: usize,
    ) -> Result<Vec<(String, f32)>> {
        // Empty query → no rows. FTS5 rejects empty MATCH.
        if lemmatized_query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT f.memory_id, bm25(memories_fts) AS score,
                    m.user_id, m.agent_id, m.run_id
             FROM memories_fts f
             JOIN memories m ON m.id = f.memory_id
             WHERE memories_fts MATCH ?1
             ORDER BY score ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![lemmatized_query, (top_k * 4).max(60) as i64],
            |row| {
                let id: String = row.get(0)?;
                let score: f32 = row.get(1)?;
                let user_id: Option<String> = row.get(2)?;
                let agent_id: Option<String> = row.get(3)?;
                let run_id: Option<String> = row.get(4)?;
                Ok((id, score, user_id, agent_id, run_id))
            },
        )?;
        let mut out: Vec<(String, f32)> = Vec::new();
        for row in rows {
            let (id, score, user_id, agent_id, run_id) = row?;
            if scope_match_field(scope.user_id.as_deref(), user_id.as_deref())
                && scope_match_field(scope.agent_id.as_deref(), agent_id.as_deref())
                && scope_match_field(scope.run_id.as_deref(), run_id.as_deref())
            {
                out.push((id, score));
                if out.len() >= top_k {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Score candidates by cosine against a query embedding and return them
    /// as `ScoredCandidate`s. Mirrors `vector_store.search` + the candidate
    /// assembly in `_search_vector_store`.
    pub(crate) fn semantic_score(
        candidates: &[StoredMemory],
        query_embedding: &[f32],
    ) -> Vec<ScoredCandidate> {
        candidates
            .iter()
            .map(|m| ScoredCandidate {
                memory_id: m.id.clone(),
                semantic_score: cosine(query_embedding, &m.embedding),
                payload_data: m.data.clone(),
                created_at: Some(m.created_at.clone()),
                updated_at: Some(m.updated_at.clone()),
                hash: Some(m.hash.clone()),
                attributed_to: m.attributed_to.clone(),
                extras: m.extras.clone(),
                scope_user_id: m.scope.user_id.clone(),
                scope_agent_id: m.scope.agent_id.clone(),
                scope_run_id: m.scope.run_id.clone(),
            })
            .collect()
    }

    /// Compute sigmoid-normalized BM25 scores keyed by memory_id. Mirrors
    /// `_search_vector_store` Step 5.
    pub(crate) fn normalize_keyword_scores(
        raw: &[(String, f32)],
        num_query_terms: usize,
    ) -> std::collections::HashMap<String, f32> {
        let (midpoint, steepness) = get_bm25_params(num_query_terms);
        raw.iter()
            .filter(|(_, s)| *s != 0.0)
            .map(|(id, raw_score)| {
                // FTS5 returns negative (lower = better); negate so higher = better.
                let positive = -raw_score;
                (id.clone(), normalize_bm25(positive, midpoint, steepness))
            })
            .collect()
    }

    /// Return the full memory record by id (no embedding). Returns
    /// `Ok(None)` if not found.
    pub fn get_memory(&self, memory_id: &str) -> Result<Option<MemoryRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, data, hash, text_lemmatized, attributed_to, user_id, agent_id,
                    run_id, actor_id, role, memory_type, expiration_date, created_at,
                    updated_at, extras
             FROM memories WHERE id = ?",
        )?;
        let mut rows = stmt.query(params![memory_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(decode_memory_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Update a memory's text/metadata by id (no LLM). Mirrors `_update_memory`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_memory_text(
        &self,
        memory_id: &str,
        text: &str,
        text_lemmatized: &str,
        hash: &str,
        embedding: &[f32],
        updated_at: &str,
        metadata_patch: Option<&str>, // optional JSON merge for extras
    ) -> Result<()> {
        let conn = self.lock();
        let blob = embed_to_blob(embedding);
        let new_extras_arg: Option<String> = metadata_patch.map(str::to_string);
        conn.execute(
            "UPDATE memories SET data = ?1, text_lemmatized = ?2, hash = ?3, embedding = ?4,
                updated_at = ?5, extras = COALESCE(?6, extras)
             WHERE id = ?7",
            params![
                text,
                text_lemmatized,
                hash,
                blob,
                updated_at,
                new_extras_arg,
                memory_id
            ],
        )?;
        Ok(())
    }

    /// Hard-delete a memory and prune its entity links. Mirrors
    /// `_delete_memory` + `_remove_memory_from_entity_store`.
    pub(crate) fn delete_memory(&self, memory_id: &str) -> Result<Option<MemoryRecord>> {
        let existing = self.get_memory(memory_id)?;
        if existing.is_none() {
            return Ok(None);
        }
        let conn = self.lock();
        conn.execute(
            "DELETE FROM entity_memory_links WHERE memory_id = ?1",
            params![memory_id],
        )?;
        // Prune entities whose link set is now empty.
        conn.execute(
            "DELETE FROM entities WHERE id NOT IN (SELECT DISTINCT entity_id FROM entity_memory_links)",
            [],
        )?;
        conn.execute("DELETE FROM memories WHERE id = ?1", params![memory_id])?;
        Ok(existing)
    }

    /// List memories for `get_all` (no scoring). Mirrors
    /// `_get_all_from_vector_store`.
    pub fn list_memories(
        &self,
        scope: &Scope,
        filter: Option<&FilterDsl>,
        limit: usize,
        show_expired: bool,
    ) -> Result<Vec<MemoryRecord>> {
        let conn = self.lock();
        let (where_clause, bindings) = scope_where(scope, show_expired);
        let sql = format!(
            "SELECT id, data, hash, text_lemmatized, attributed_to, user_id, agent_id,
                    run_id, actor_id, role, memory_type, expiration_date, created_at,
                    updated_at, extras
             FROM memories {where_clause}
             ORDER BY created_at DESC LIMIT ?"
        );
        let mut all_bindings: Vec<Box<dyn rusqlite::ToSql>> = bindings;
        all_bindings.push(Box::new(limit.to_string()));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(all_bindings.iter()), |r| {
            decode_memory_row(r)
        })?;
        let mut out = Vec::new();
        for row in rows {
            let stored = row?;
            if let Some(f) = filter
                && !f.matches(&stored.extras)
            {
                continue;
            }
            out.push(stored);
        }
        Ok(out)
    }

    // ----- history -----------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_history(
        &self,
        memory_id: &str,
        old_memory: Option<&str>,
        new_memory: Option<&str>,
        event: &str,
        created_at: &str,
        updated_at: Option<&str>,
        actor_id: Option<&str>,
        role: Option<&str>,
        is_deleted: bool,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO history (id, memory_id, old_memory, new_memory, event,
                created_at, updated_at, is_deleted, actor_id, role)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                uuid_str(),
                memory_id,
                old_memory,
                new_memory,
                event,
                created_at,
                updated_at,
                is_deleted as i64,
                actor_id,
                role,
            ],
        )?;
        Ok(())
    }

    pub fn history(&self, memory_id: &str) -> Result<Vec<HistoryEvent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, memory_id, old_memory, new_memory, event, created_at,
                    updated_at, is_deleted, actor_id, role
             FROM history WHERE memory_id = ?
             ORDER BY created_at ASC, updated_at ASC",
        )?;
        let rows = stmt.query_map(params![memory_id], |row| {
            Ok(HistoryEvent {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                old_memory: row.get(2)?,
                new_memory: row.get(3)?,
                event: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                is_deleted: row.get::<_, i64>(7)? != 0,
                actor_id: row.get(8)?,
                role: row.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ----- entities ----------------------------------------------------

    /// Insert a new memory→entity edge. Creates the entity if missing.
    /// Mirrors `_upsert_entity` + Phase 7 batch logic.
    pub(crate) fn upsert_entity(
        &self,
        candidate: &EntityCandidate,
        memory_id: &str,
        scope: &Scope,
        embedding: &[f32],
    ) -> Result<()> {
        let conn = self.lock();
        let normalized = candidate.normalized();
        // Exact-text dedup within scope.
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM entities
                 WHERE data_normalized = ?1
                   AND COALESCE(user_id,'') = COALESCE(?2,'')
                   AND COALESCE(agent_id,'') = COALESCE(?3,'')
                   AND COALESCE(run_id,'') = COALESCE(?4,'')",
                params![normalized, scope.user_id, scope.agent_id, scope.run_id],
                |row| row.get(0),
            )
            .optional()?;
        let entity_id = match existing_id {
            Some(id) => id,
            None => {
                let id = uuid_str();
                let blob = embed_to_blob(embedding);
                conn.execute(
                    "INSERT INTO entities (id, data, data_normalized, entity_type, embedding,
                        user_id, agent_id, run_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        candidate.text,
                        normalized,
                        entity_type_str(candidate.entity_type),
                        blob,
                        scope.user_id,
                        scope.agent_id,
                        scope.run_id,
                    ],
                )?;
                id
            }
        };
        // Insert the link (idempotent).
        conn.execute(
            "INSERT OR IGNORE INTO entity_memory_links (entity_id, memory_id) VALUES (?1, ?2)",
            params![entity_id, memory_id],
        )?;
        Ok(())
    }

    /// Find entities whose embedding cosine-similarity to `query_entities`
    /// exceeds `floor` (default 0.5). Returns `(memory_id, max_boost)`
    /// contributions keyed by memory. Mirrors `_compute_entity_boosts`.
    pub(crate) fn entity_boosts(
        &self,
        query_embeddings: &[(EntityCandidate, Vec<f32>)],
        scope: &Scope,
        floor: f32,
    ) -> Result<std::collections::HashMap<String, f32>> {
        // Load every in-scope entity + its embedding + linked memory ids once.
        let conn = self.lock();
        let (where_clause, bindings) = entity_scope_where(scope);
        let sql = format!(
            "SELECT e.id, e.embedding
             FROM entities e {where_clause}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut entities: Vec<(String, Vec<f32>)> = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;
        for row in rows {
            let (id, blob) = row?;
            entities.push((id, blob_to_embed(&blob)));
        }
        drop(stmt);

        // Pull all entity→memory links once into a map.
        let mut links: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut link_stmt = conn.prepare("SELECT entity_id, memory_id FROM entity_memory_links")?;
        let link_rows = link_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in link_rows {
            let (entity_id, memory_id) = row?;
            links.entry(entity_id).or_default().push(memory_id);
        }
        drop(link_stmt);
        drop(conn);

        // For each query entity, scan entities in Rust and apply the boost formula.
        let mut boosts: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for (_candidate, qemb) in query_embeddings {
            if qemb.is_empty() {
                continue;
            }
            for (entity_id, emb) in &entities {
                let sim = cosine(qemb, emb);
                if sim < floor {
                    continue;
                }
                if let Some(memory_ids) = links.get(entity_id) {
                    let n = memory_ids.len().max(1);
                    let weight = 1.0 / (1.0 + 0.001 * ((n - 1) as f32).powi(2));
                    let boost = sim * crate::bm25::ENTITY_BOOST_WEIGHT * weight;
                    for mid in memory_ids {
                        let entry = boosts.entry(mid.clone()).or_insert(0.0);
                        if boost > *entry {
                            *entry = boost;
                        }
                    }
                }
            }
        }
        Ok(boosts)
    }

    /// Drop all data (history, messages, memories, entities, links).
    pub fn reset(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch(
            "DELETE FROM entity_memory_links;
             DELETE FROM entities;
             DELETE FROM memories_fts;
             DELETE FROM memories;
             DELETE FROM messages;
             DELETE FROM history;",
        )?;
        Ok(())
    }
}

// ----- helpers -----------------------------------------------------------

pub(crate) struct NewMemory<'a> {
    pub id: &'a str,
    pub data: &'a str,
    pub hash: &'a str,
    pub text_lemmatized: &'a str,
    pub embedding: &'a [f32],
    pub attributed_to: Option<Attribution>,
    pub scope: &'a Scope,
    pub actor_id: Option<&'a str>,
    pub role: Option<&'a str>,
    pub memory_type: Option<&'a str>,
    pub expiration_date: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub extras: &'a std::collections::BTreeMap<String, Value>,
}

/// Decoder for queries that include the embedding column (column index 4 =
/// embedding blob). Used by `list_for_semantic_search`.
fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMemory> {
    let id: String = row.get(0)?;
    let data: String = row.get(1)?;
    let hash: String = row.get(2)?;
    let text_lemmatized: String = row.get(3)?;
    let blob: Vec<u8> = row.get(4)?;
    let embedding = blob_to_embed(&blob);
    let attributed_to: Option<String> = row.get(5)?;
    let user_id: Option<String> = row.get(6)?;
    let agent_id: Option<String> = row.get(7)?;
    let run_id: Option<String> = row.get(8)?;
    let actor_id: Option<String> = row.get(9)?;
    let role: Option<String> = row.get(10)?;
    let memory_type: Option<String> = row.get(11)?;
    let expiration_date: Option<String> = row.get(12)?;
    let created_at: String = row.get(13)?;
    let updated_at: String = row.get(14)?;
    let extras_json: String = row.get(15)?;
    let extras: std::collections::BTreeMap<String, Value> =
        serde_json::from_str(&extras_json).unwrap_or_default();
    Ok(StoredMemory {
        id,
        data,
        hash,
        text_lemmatized,
        embedding,
        attributed_to,
        actor_id,
        role,
        memory_type,
        expiration_date,
        created_at,
        updated_at,
        extras,
        scope: Scope {
            user_id,
            agent_id,
            run_id,
        },
    })
}

/// Decoder for queries that omit the embedding column (column index 4 =
/// attributed_to). Used by `get_memory` and `list_memories`.
fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let id: String = row.get(0)?;
    let data: String = row.get(1)?;
    let hash: String = row.get(2)?;
    let text_lemmatized: String = row.get(3)?;
    let attributed_to: Option<String> = row.get(4)?;
    let user_id: Option<String> = row.get(5)?;
    let agent_id: Option<String> = row.get(6)?;
    let run_id: Option<String> = row.get(7)?;
    let actor_id: Option<String> = row.get(8)?;
    let role: Option<String> = row.get(9)?;
    let memory_type: Option<String> = row.get(10)?;
    let expiration_date: Option<String> = row.get(11)?;
    let created_at: String = row.get(12)?;
    let updated_at: String = row.get(13)?;
    let extras_json: String = row.get(14)?;
    let extras: std::collections::BTreeMap<String, Value> =
        serde_json::from_str(&extras_json).unwrap_or_default();
    Ok(MemoryRecord {
        id,
        data,
        hash,
        text_lemmatized,
        attributed_to: attributed_to.as_deref().and_then(Attribution::parse),
        scope: Scope {
            user_id,
            agent_id,
            run_id,
        },
        actor_id,
        role,
        memory_type,
        expiration_date,
        created_at,
        updated_at,
        extras,
    })
}

fn decode_memory_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    // Legacy decoder for the no-embedding SELECT shape; preserved for
    // backwards compatibility. Prefer `row_to_record` for new call sites.
    row_to_record(row)
}

/// Build the WHERE clause and bindings for a scope-filtered memories query.
/// Skips expired rows unless `show_expired`.
fn scope_where(scope: &Scope, show_expired: bool) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut clauses = Vec::new();
    let mut bindings: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(v) = &scope.user_id {
        clauses.push("user_id = ?");
        bindings.push(Box::new(v.clone()));
    }
    if let Some(v) = &scope.agent_id {
        clauses.push("agent_id = ?");
        bindings.push(Box::new(v.clone()));
    }
    if let Some(v) = &scope.run_id {
        clauses.push("run_id = ?");
        bindings.push(Box::new(v.clone()));
    }
    if !show_expired {
        clauses.push("(expiration_date IS NULL OR expiration_date >= date('now'))");
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_clause, bindings)
}

fn entity_scope_where(scope: &Scope) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut clauses = Vec::new();
    let mut bindings: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(v) = &scope.user_id {
        clauses.push("user_id = ?");
        bindings.push(Box::new(v.clone()));
    }
    if let Some(v) = &scope.agent_id {
        clauses.push("agent_id = ?");
        bindings.push(Box::new(v.clone()));
    }
    if let Some(v) = &scope.run_id {
        clauses.push("run_id = ?");
        bindings.push(Box::new(v.clone()));
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_clause, bindings)
}

#[allow(dead_code)]
fn scope_token(_scope: &Scope) -> String {
    String::new()
}

/// Compare a query scope field against a stored scope field. `None` on the
/// stored side means "any"; `None` on the query side means "no constraint".
fn scope_match_field(query: Option<&str>, stored: Option<&str>) -> bool {
    match (query, stored) {
        (Some(q), Some(s)) => q == s,
        (Some(q), None) => q.is_empty(),
        (None, _) => true,
    }
}

fn entity_type_str(t: crate::nlp::EntityType) -> &'static str {
    match t {
        crate::nlp::EntityType::Proper => "PROPER",
        crate::nlp::EntityType::Quoted => "QUOTED",
        crate::nlp::EntityType::Topic => "TOPIC",
        crate::nlp::EntityType::Identifier => "IDENTIFIER",
    }
}

fn embed_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for v in embedding {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

fn blob_to_embed(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
            f32::from_le_bytes(arr)
        })
        .collect()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn uuid_str() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Custom SQL function registered on init that mirrors scope-matching in
/// Rust. We expose this so the FTS join can filter by scope inline.
#[allow(dead_code)]
pub(crate) fn register_scope_match(_conn: &Connection) -> rusqlite::Result<()> {
    // v1 filters scope in Rust after the FTS query (see `keyword_search`).
    // The custom-function route is reserved for a future optimization.
    Ok(())
}

impl CognitiveStore {
    /// Open with custom SQL functions registered. The TUI binary uses this
    /// entry point so the FTS keyword-search scope filter works.
    pub fn open_with_functions(path: &Path) -> Result<Self> {
        // v1 filters scope in Rust; `open_with_functions` is kept as the
        // canonical entry point so a future custom-function optimization
        // can slot in without changing the composition boundary.
        Self::open(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        CognitionPorts, EmbedAction, EmbeddingPort, EntityExtractorPort, ExtractionLlmPort,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Deterministic stub embedder: hashes text to a small fixed-dim vector.
    /// Used in tests so we never touch a real provider.
    struct StubEmbedder(usize);
    impl EmbeddingPort for StubEmbedder {
        fn embed(&self, text: &str, _action: EmbedAction) -> Result<Vec<f32>> {
            let mut out = vec![0.0_f32; self.0];
            for (i, b) in text.bytes().enumerate() {
                out[i % self.0] += (b as f32) / 255.0;
            }
            // Normalize to unit length so cosine is meaningful.
            let norm = out.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut out {
                    *v /= norm;
                }
            }
            Ok(out)
        }
    }

    /// Stub extractor that returns a fixed set of memories per call. The
    /// test sets the response via a `Mutex<String>` so the test can change
    /// it between calls.
    struct StubExtractor(std::sync::Mutex<String>);
    impl ExtractionLlmPort for StubExtractor {
        fn extract(&self, _system: &str, _user: &str) -> Result<String> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    /// In-crate default entity extractor (regex fallback).
    struct DefaultEntities;
    impl EntityExtractorPort for DefaultEntities {
        fn extract(&self, text: &str) -> Vec<EntityCandidate> {
            crate::nlp::extract_entities(text)
        }
    }

    /// Returns a `(TempDir, PathBuf)` so the directory stays alive for the
    /// test's lifetime. (Returning only the path leaks/drops the dir.)
    fn tmp_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cognition.db");
        (dir, path)
    }

    fn ports(dim: usize, response: &str) -> CognitionPorts {
        CognitionPorts {
            embedder: Arc::new(StubEmbedder(dim)),
            extractor: Arc::new(StubExtractor(std::sync::Mutex::new(response.to_string()))),
            entity_nlp: Arc::new(DefaultEntities),
        }
    }

    #[test]
    fn hash_exists_round_trip() {
        let (_dir, db_path) = tmp_db();
        let store = CognitiveStore::open_with_functions(&db_path).unwrap();
        let _ = &_dir;
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let emb = vec![0.1, 0.2, 0.3];
        let extras = BTreeMap::new();
        let now = now_iso();
        let id = uuid::Uuid::new_v4().to_string();
        let new_mem = NewMemory {
            id: &id,
            data: "User likes Rust",
            hash: "abc",
            text_lemmatized: "user like rust",
            embedding: &emb,
            attributed_to: None,
            scope: &scope,
            actor_id: None,
            role: None,
            memory_type: None,
            expiration_date: None,
            created_at: &now,
            updated_at: &now,
            extras: &extras,
        };
        store.insert_memory(&new_mem).unwrap();
        assert!(store.hash_exists("abc", &scope).unwrap());
        assert!(!store.hash_exists("xyz", &scope).unwrap());
    }

    #[test]
    fn keyword_search_returns_fts5_bm25() {
        let (_dir, db_path) = tmp_db();
        let store = CognitiveStore::open_with_functions(&db_path).unwrap();
        let _ = &_dir;
        let scope = Scope {
            user_id: Some("u1".into()),
            ..Scope::default()
        };
        let emb = vec![0.1_f32; 4];
        let now = now_iso();
        let extras = BTreeMap::new();
        for (id, lemmatized) in [("m1", "rust memory engine"), ("m2", "python web server")] {
            let new_mem = NewMemory {
                id,
                data: lemmatized,
                hash: id,
                text_lemmatized: lemmatized,
                embedding: &emb,
                attributed_to: None,
                scope: &scope,
                actor_id: None,
                role: None,
                memory_type: None,
                expiration_date: None,
                created_at: &now,
                updated_at: &now,
                extras: &extras,
            };
            store.insert_memory(&new_mem).unwrap();
        }
        let hits = store.keyword_search("rust engine", &scope, 10).unwrap();
        // Only the rust memory should match.
        let matched_ids: Vec<&str> = hits.iter().map(|(id, _)| id.as_str()).collect();
        assert!(matched_ids.contains(&"m1"));
        assert!(!matched_ids.contains(&"m2"));
    }

    #[test]
    fn rolling_window_evicts_old_messages() {
        let (_dir, db_path) = tmp_db();
        let store = CognitiveStore::open_with_functions(&db_path).unwrap();
        let _ = &_dir;
        let scope_key = "user_id=u1";
        for i in 0..(ROLLING_MESSAGE_LIMIT + 5) {
            store
                .save_messages(
                    &[Message {
                        role: "user".into(),
                        content: format!("msg-{i}"),
                        name: None,
                    }],
                    scope_key,
                )
                .unwrap();
            // Ensure monotonic timestamps so the ORDER BY picks the latest.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let last = store
            .get_last_messages(scope_key, ROLLING_MESSAGE_LIMIT)
            .unwrap();
        assert_eq!(last.len(), ROLLING_MESSAGE_LIMIT);
        // Latest message must be present; first message must be evicted.
        assert!(last.iter().any(|m| m.content == "msg-14"));
        assert!(!last.iter().any(|m| m.content == "msg-0"));
    }

    // Sanity that ports construct under the stub impls (no compile surface check needed).
    #[test]
    fn ports_bundle_constructs_with_stubs() {
        let _ = ports(8, "{\"memory\":[]}");
    }
}
