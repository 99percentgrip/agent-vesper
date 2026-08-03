# mem0 Cognitive Memory — Local Reconnaissance & Rust Blueprint

Status: COMPLETE.

Source oracle inspected: `/home/alex/Projects/mem0/` — git pin
`29fa41558cf33263ec961dd9c6ff4245182466ef` ("April 2026 new algorithm" release).
Target host crate: new sibling `crates/vesper-cognition` (see ADR 0015).
Companion evidence: [`sqlite-fts5-spike.md`](sqlite-fts5-spike.md) (rusqlite
0.40.1 bundled validated for FTS5 across release targets).

This document is the durable reconnaissance record that ADR 0015 builds on.
It is **planning-only**: no Rust crates, production code, speculative
dependencies, or copied Python modules were added during reconnaissance. Every
current-state claim cites a `file:line` source path and symbol in the pinned
oracle. Behavior is labeled **confirmed / inferred / gap**.

## 0. Architectural verdict

| Question | Answer (all **confirmed** unless noted) |
|---|---|
| Algorithm class | Single-pass ADD-only extraction (V3). One LLM call per `add()`. No UPDATE/DELETE in primary path. |
| New-vs-update-vs-contradiction decision | No per-fact decision LLM in V3. V2's `DEFAULT_UPDATE_MEMORY_PROMPT` ships but is dead code. Deduplication is post-hoc MD5(text). |
| Storage topology | Three stores: vector store (source of truth), SQLite `history_db` (audit + 10-message window), entity store (sibling collection). |
| Retrieval | Additive fusion: `score = (semantic + bm25_normalized + entity_boost) / max_possible`. Sigmoid BM25, query-length-adaptive. |
| Hard deps | spaCy (lemmatization + NER), embedding provider, LLM provider. |

Vesper port substitutes SQLite FTS5 for the BM25 sparse-vector sidecar,
Snowball (`rust-stemmers`) for spaCy lemmatization, regex heuristics for spaCy
NER, and trait ports for embedding/LLM (concrete impls at the composition
boundary). Parity gaps are documented in §7.7 of the inline report and the ADR.

The full structural blueprint (file layout, port traits, SQLite schema, public
API sketch, parity-gap table, evidence index) is recorded verbatim in the
session that produced this document and is ratified by ADR 0015. The remainder
of this file records only what ADR 0015 needs to cite as evidence.

## 1. V3 algorithm — confirmed facts

- **Categories** (`mem0/configs/enums.py:3`): `SEMANTIC`, `EPISODIC`,
  `PROCEDURAL`. Only `PROCEDURAL` is special-cased in code
  (`_create_procedural_memory`, `memory/main.py:1949`). The other two are enum
  values with no code path in V3 OSS — there is no `category` column on a
  stored memory.
- **V3 8-phase `add()` pipeline** (`memory/main.py:849-1178`):
  0. context gathering (10-message SQLite window);
  1. existing-memory retrieval (`vector_store.search(top_k=10)`), UUIDs mapped
     to integers before the LLM sees them (anti-hallucination);
  2. single LLM extraction call with `response_format=json_object`;
  3. batch embed;
  4+5. per-memory MD5 dedup (existing-hash ∪ within-batch-hash) + lemmatize;
  6. batch `vector_store.insert` + `db.batch_add_history`;
  7. batch entity linking (semantic ≥0.95 OR exact text → upsert);
  8. `db.save_messages` + return ADD events.
- **Hash dedup** (`memory/main.py:1041-1059`): `mem_hash = md5(text)`. Skip if
  `mem_hash ∈ existing_hashes ∪ seen_hashes`. **Two memories with even slightly
  different text both survive**; contradictions are stored as competing
  memories.
- **Memory payload** (Phase 4): `{data, hash, text_lemmatized, created_at,
  updated_at, attributed_to?, **filters, **user_metadata}`. The
  `linked_memory_ids` the LLM emits is **not persisted** in OSS V3 — only
  entity→memory edges are.
- **Failure semantics** (`memory/main.py:955`): the LLM call re-raises as
  `LLMError` so 429/5xx are distinguishable from "nothing extracted."

## 2. V3 retrieval — confirmed math

- `search()` (`memory/main.py:1349`): `top_k=20, threshold=0.1`, optional
  reranker applied after hybrid scoring.
- `_search_vector_store` (`main.py:1598`): over-fetch `max(top_k*4, 60)`, run
  semantic + keyword(BM25) + entity-boost in parallel, fuse additively.
- **BM25 sigmoid** (`utils/scoring.py`):
  `normalize_bm25(raw, midpoint, steepness) = 1/(1+exp(-steepness*(raw-midpoint)))`.
  Query-length-adaptive params (5 rows: ≤3 → (5.0, 0.7); ≤6 → (7.0, 0.6);
  ≤9 → (9.0, 0.5); ≤15 → (10.0, 0.5); else (12.0, 0.5)).
- **Hybrid scoring** (`score_and_rank`):
  `combined = min((semantic + bm25 + entity_boost) / max_possible, 1.0)` where
  `max_possible ∈ {1.0, 1.5, 2.0, 2.5}` adapts to which signals are active
  (semantic-only, +entity, +bm25, +both). Threshold gates semantic BEFORE
  combining.
- **Entity boost** (`main.py:1703`): per query entity (deduped, max 8),
  `entity_store.search(top_k=500, score≥0.5)`; for each match,
  `boost = similarity * 0.5 * memory_count_weight` where
  `memory_count_weight = 1/(1+0.001*(n-1)^2)`. **boost ∈ [0, 0.5]** per memory.

## 3. SQLite history schema — confirmed (`memory/storage.py`)

```sql
CREATE TABLE history (id TEXT PK, memory_id TEXT, old_memory TEXT,
  new_memory TEXT, event TEXT, created_at DATETIME, updated_at DATETIME,
  is_deleted INTEGER, actor_id TEXT, role TEXT);
CREATE TABLE messages (id TEXT PK, session_scope TEXT, role TEXT,
  content TEXT, name TEXT, created_at DATETIME);
-- messages capped at 10 per session_scope via post-insert DELETE.
```

Critical: the **memory content itself never lives in SQLite** — it lives in the
vector-store payload. SQLite stores only the audit log + rolling context.

## 4. Honest parity gaps (ratified by ADR 0015)

| Capability | Oracle | Vesper port | Parity |
|---|---|---|---|
| Lemmatization | spaCy `en_core_web_sm` | `rust-stemmers` Snowball (English) | Partial — slight BM25 quality loss on verb-form edge cases. |
| Entity extraction | spaCy NER + 700 LOC heuristics | Regex heuristics (PROPER/QUOTED/IDENTIFIER/TOPIC) | Partial — TOPIC weaker; PROPER coarser. |
| Embeddings | OpenAI `text-embedding-3-small` 1536-d | Trait port; concrete impl at composition boundary (Zai `embedding-3` 1024-d) | Full (provider-routed). |
| Extraction LLM | OpenAI chat with `response_format=json_object` | Trait port; concrete impl at composition boundary (Zai chat-completions) | Full with JSON-extraction regex fallback. |
| Reranker | Cohere/HF/ST/LLM/ZeroEntropy | Out of scope for v1 | Deferred (oracle defaults to off). |
| V2 UPDATE/DELETE LLM | `DEFAULT_UPDATE_MEMORY_PROMPT` (dead in V3) | Not ported — V3 is ADD-only | Full (matches oracle). |
| Procedural memory | `PROCEDURAL_MEMORY_SYSTEM_PROMPT` | Ported verbatim | Full. |
| Memory→memory `linked_memory_ids` | Emitted by LLM, not persisted | Skip persistence | Full (matches OSS). |
| 25 vector-store backends | Qdrant/Chroma/PGVector/… | SQLite-only | Intentional scope reduction (Vesper: no external services in foundation). |

## 5. Default configuration (`mem0/configs/base.py:29`)

| Knob | Default |
|---|---|
| `vector_store.provider` | `qdrant` (Vesper: SQLite) |
| `embedder.provider` / model / dims | `openai` / `text-embedding-3-small` / 1536 (Vesper: configurable; Zai default 1024) |
| `llm.provider` | `openai` (Vesper: provider-routed trait) |
| `history_db_path` | `$HOME/.mem0/history.db` |
| Rolling message window | 10 per session_scope |
| Existing-memory top-k for extraction | 10 |
| BM25 internal_limit | `max(top_k*4, 60)` |
| Entity boost similarity floor | 0.5 |
| Entity semantic-dedup threshold | 0.95 |
| Memory dedup | MD5(text) |
| Past-message truncation | 300 chars |

## 6. Evidence index (oracle `29fa4155` file:line)

| Topic | Source |
|---|---|
| V3 algorithm overview | `README.md` "New Memory Algorithm (April 2026)" |
| MemoryType enum | `mem0/configs/enums.py:3` |
| ADDITIVE_EXTRACTION_PROMPT | `mem0/configs/prompts.py:468-945` |
| AGENT_CONTEXT_SUFFIX | `mem0/configs/prompts.py:947` |
| `generate_additive_extraction_prompt` | `mem0/configs/prompts.py:1016` |
| DEFAULT_UPDATE_MEMORY_PROMPT (V2, dead) | `mem0/configs/prompts.py:176` |
| PROCEDURAL_MEMORY_SYSTEM_PROMPT | `mem0/configs/prompts.py:326` |
| V3 8-phase add pipeline | `mem0/memory/main.py:849-1178` |
| UUID→integer anti-hallucination | `mem0/memory/main.py:917-921` |
| MD5 hash dedup | `mem0/memory/main.py:1041-1059` |
| Batch entity linking | `mem0/memory/main.py:1083-1170` |
| `search()` public API | `mem0/memory/main.py:1349-1494` |
| `_search_vector_store` hybrid pipeline | `mem0/memory/main.py:1598-1703` |
| `_compute_entity_boosts` | `mem0/memory/main.py:1703-1784` |
| `score_and_rank` / `normalize_bm25` / `get_bm25_params` | `mem0/utils/scoring.py` |
| `lemmatize_for_bm25` (spaCy) | `mem0/utils/lemmatization.py` |
| `extract_entities` / `extract_entities_batch` | `mem0/utils/entity_extraction.py:751, 761` |
| ENTITY_BOOST_WEIGHT = 0.5 | `mem0/utils/scoring.py:60` |
| SQLiteManager schema | `mem0/memory/storage.py` |
| VectorStoreBase / EmbeddingBase interfaces | `mem0/vector_stores/base.py`, `mem0/embeddings/base.py` |
| `_build_filters_and_metadata` / `_build_session_scope` | `mem0/memory/main.py:301, 387` |
| Default Qdrant BM25 sparse-vector impl | `mem0/vector_stores/qdrant.py:86-121` |
