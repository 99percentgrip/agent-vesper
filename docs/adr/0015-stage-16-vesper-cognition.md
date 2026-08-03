# ADR 0015: Stage 16 — vesper-cognition (Local mem0 Cognitive Memory)

Status: ACCEPTED

Builds on: [ADR 0014](0014-agent-vesper-authentication.md),
[ADR 0011](0011-stage-12-persistent-memory-and-awareness.md),
[ADR 0013](0013-stage-15-mcp-client-and-plugin-loader.md).

Evidence: [mem0-cognitive-memory-blueprint.md](../foundation/mem0-cognitive-memory-blueprint.md),
[sqlite-fts5-spike.md](../foundation/sqlite-fts5-spike.md).

## Context

The lead architect authorized Stage 16: a native Rust cognitive memory engine
emulating the local mem0 V3 oracle at `/home/alex/Projects/mem0/` (git pin
`29fa41558cf33263ec961dd9c6ff4245182466ef`, the "April 2026 new algorithm"
release). The reconnaissance blueprint documents the V3 architecture in full.

### Why mem0 must not live in `vesper-memory`

`vesper-memory` (ADR 0011 — Stage 12) owns the durable memory graph, learned
skills, user profile, and bounded awareness ledger. Its crate contract is
explicit: it **depends only on `vesper-domain` and `vesper-security`**, with
**no SQLite, HTTP, or TUI dependency**. mem0's V3 algorithm needs all three:

- **SQLite** for the relational audit log + FTS5 BM25 index + entity graph.
- **HTTP** for the embedding + extraction-LLM provider calls.
- **Embeddings + LLM** as runtime services.

Putting this in `vesper-memory` would violate ADR 0011 and entangle the
existing skill/profile/awareness storage with vector-index concerns. The
recon blueprint identifies two options; this ADR ratifies **Option A** (a
dedicated sibling crate), mirroring the established pattern of `vesper-sessions`
(SQLite owner, read-mostly), `vesper-mcp` (Ed25519-only plugin loader), and
`vesper-checkpoints` (workspace snapshots).

### Oracle data model (audit)

The V3 oracle is fundamentally different from V2:

- **Single-pass ADD-only extraction.** One LLM call per `add()` (`mem0/memory/main.py:849-1178`).
  The LLM emits only `ADD`; the V2 `DEFAULT_UPDATE_MEMORY_PROMPT`
  ADD/UPDATE/DELETE/NONE decision LLM is dead code in V3.
- **Post-hoc MD5 deduplication.** `mem_hash = md5(text)`; skip if hash
  duplicates an existing or in-batch memory (`memory/main.py:1041-1059`). Two
  memories with even slightly different text both survive; contradictions are
  stored as competing memories and resolved at retrieval time.
- **Hybrid retrieval.** `score = (semantic + bm25_normalized + entity_boost) /
  max_possible` where `max_possible ∈ {1.0, 1.5, 2.0, 2.5}` adapts to which
  signals are active (`mem0/utils/scoring.py`).
- **Entity linking.** Per-memory entity extraction + a sibling entity-store
  collection with `linked_memory_ids[]`; entity-boost formula bounded to
  `[0, 0.5]` per memory with a `1/(1+0.001*(n-1)^2)` hyper-connection penalty
  (`memory/main.py:1703`).

### Why SQLite is the right Vesper backing

The reconnaissance confirmed that **mem0's memory content lives in the vector
store payload, not in SQLite**. SQLite stores only the audit log + a 10-message
rolling context window. For a Vesper port that owns no external vector-index
service (Vesper's foundation never depends on external services), SQLite is the
natural single embedded backing for **all three** concerns: relational payload,
FTS5 BM25 over lemmatized text, and embedding-as-BLOB with cosine computed in
Rust. The `sqlite-fts5-spike.md` validates `rusqlite 0.40.1` (bundled) for
reliable FTS5 across all five release targets.

## Decision

1. **New crate `vesper-cognition`** owns the V3 cognitive memory engine:
   - The 8-phase ADD-only extraction pipeline.
   - Hybrid retrieval (semantic cosine + FTS5 BM25 + entity boost) with the
     oracle's exact scoring math.
   - The SQLite relational + FTS5 schema (history, messages, memories,
     memories_fts, entities, entity_memory_links).
   - The Snowball (`rust-stemmers`) lemmatization fallback in place of spaCy.
   - Regex-based entity extraction (PROPER / QUOTED / TOPIC / IDENTIFIER).
   - Three trait ports: `EmbeddingPort`, `ExtractionLlmPort`,
     `EntityExtractorPort`. **Concrete implementations are forbidden in this
     crate.** They are constructed at the composition boundary
     (`apps/agent-vesper-tui/src/main.rs`) using the existing Zai adapter.

2. **Strict dependency surface.** `vesper-cognition` depends only on
   `vesper-domain`, `vesper-security`, `rusqlite` (bundled), `rust-stemmers`,
   and the standard utility crates already in `[workspace.dependencies]`
   (`serde`, `serde_json`, `thiserror`, `chrono`, `uuid`, `md-5`, `regex`).
   **No provider, runtime, ACP, sessions, agent, testkit, HTTP, or TUI
   dependency.** `#![forbid(unsafe_code)]` is enforced at the crate root.

3. **xtask architecture rule amendment.** `cargo xtask architecture`
   historically hard-prohibited `rusqlite`/`sqlx`/`libsqlite3-sys` in
   production crates (a "Stage 5" holdover) and listed `rusqlite` in
   `shared_forbidden` for the source scanner. This ADR amends that rule:
   - Adds `vesper-cognition` to `allowed_dependencies` with
     `[vesper-domain, vesper-security]`.
   - Replaces the blanket SQLite prohibition with a per-crate allowlist
     exception: only `vesper-cognition` may declare `rusqlite`. Every other
     production crate (including `vesper-sessions`, which intentionally has no
     SQLite index per its AGENTS.md) remains SQLite-free.
   - Adds `vesper-cognition` to the per-crate exception map in
     `scan_production_sources` so the source scanner permits the `rusqlite`
     import only there.
   - Flips `workspace.metadata.agent-vesper.sqlite-enabled` to `true`.

4. **Composition boundary wiring.** The TUI binary owns a `CognitionBundle`
   that constructs `CognitionPorts` (the three trait impls) using
   `vesper_provider_glm::auth::resolve_credential` + a blocking reqwest client
   (consistent with the binary's existing pattern of performing blocking I/O
   on Tokio threads). The Zai adapter does not currently expose a public
   embeddings or sync chat-completion API; the trait impls therefore live in
   the binary and call the real Zai endpoints
   (`/api/paas/v4/embeddings` for `embedding-3` 1024-d; `/api/paas/v4/chat/completions`
   with `response_format=json_object`). A JSON-extraction regex fallback mirrors
   the oracle's `extract_json` resilience for providers that do not honor
   `response_format=json_object`.

5. **`vesper-memory` stays pure.** ADR 0011's contract is unchanged. The
   existing `/memory`, `/skills`, `/profile`, `/awareness`, `/goal`,
   `/subgoal`, `/deliberation`, `/metacognition`, `/repository`,
   `/meta-learning`, `/observability`, `/curator`, `/journey` commands remain
   backed by the append-only JSONL + markdown stores. `vesper-cognition` adds
   a **new** cognitive surface; it does not replace `vesper-memory`.

## Parity scope (faithful v1)

This ADR ratifies the parity-gap table in the reconnaissance blueprint:

- **Full parity**: V3 ADD-only pipeline, MD5 dedup, 8-phase orchestration,
  hybrid scoring math (semantic + BM25 sigmoid + entity boost with hyper-
  connection penalty), BM25 query-length-adaptive params, filter DSL
  (eq/ne/in/nin/gt/gte/lt/lte/contains/icontains/wildcard/AND/OR/NOT), 10-message
  rolling context window, procedural-memory compaction, admin `update`/`delete`
  (no LLM), history audit log.
- **Partial parity (Snowball fallback)**: lemmatization (slight BM25 quality
  loss on verb forms vs spaCy); entity extraction (regex PROPER/QUOTED/
  IDENTIFIER + coarse TOPIC; spaCy-grade accuracy requires a future Rust NLP
  crate or Python sidecar).
- **Deferred (out of v1)**: rerankers (oracle defaults to off); 25 vector-store
  backends (Vesper is SQLite-only by design — no external services in
  foundation); memory→memory `linked_memory_ids` persistence (matches OSS V3).

## Compatibility consequences

- **Workspace**: gains one new member (`crates/vesper-cognition`).
- **Workspace metadata**: `sqlite-enabled = false` → `true`.
- **xtask**: `allowed_dependencies` gains `vesper-cognition`;
  `scan_production_sources` gains a per-crate `rusqlite` exception for
  `vesper-cognition` only; the blanket Stage-5 SQLite prohibition is replaced
  by a per-crate allowlist.
- **`vesper-memory`**: unchanged contract.
- **TUI**: gains a `CognitionBundle` and a `drain_cognition_op` executor that
  resolves `SessionState.pending_cognition_op` after each dispatch (mirrors
  the Stage 12 `drain_memory_op` pattern).
- **Cargo.lock**: gains `rusqlite` (transitively `libsqlite3-sys` bundled,
  `bundled-sqlite`), `rust-stemmers`.

## Security consequences

- All filesystem writes are confined to the absolute root passed at
  construction. Refuses a non-absolute root or a root whose parent does not
  exist (same confinement rule as the Stage 6 writer and `vesper-memory`).
- The crate never creates the root directory; the composition boundary owns
  that. SQLite is opened with `SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE` at
  the configured path only — no network, no subprocess.
- Provider-bound secrets (Zai API key) never enter the crate. They are
  resolved at the composition boundary and used by the trait impls in the
  binary; the crate sees only opaque `&[u8]` text and produces `Vec<f32>`
  vectors.
- No `unsafe` code is permitted (`#![forbid(unsafe_code)]`). rusqlite's
  bundled SQLite amalgamation is itself C and is outside this crate's
  `unsafe_code` ban (it lives in `libsqlite3-sys`, not in `vesper-cognition`).
- `CognitionError` messages are sanitized: never include file contents, API
  keys, full paths, or extracted memory text in error text.

## Migration consequences

- No data migration. `vesper-memory`'s existing JSONL store is untouched.
- The first run of a TUI binary that constructs a `CognitionBundle` creates a
  new SQLite database under the configured cognition root. Failure to open is
  non-fatal: the bundle degrades to `None` and the TUI continues without
  cognitive memory (mirrors how `MemoryStores::open_default` already
  `.ok().map(Arc::new)`-tolerates each store).
- This is the first production crate to introduce SQLite. The
  `sqlite-fts5-spike.md` five-target CI matrix remains the validation
  reference.

## Verification

- `cargo test -p vesper-cognition` — pipeline tests mirroring the oracle's 12
  prompt examples, MD5 dedup regression, BM25 sigmoid parametrization (5 rows),
  hybrid scoring divisor adapts to active signals (1.0 / 1.5 / 2.0 / 2.5),
  entity-boost bounds (≤0.5; memory-count-weight curve), Filter DSL operators,
  rolling 10-message window eviction, procedural-memory single-LLM-call flow,
  cosine similarity, FTS5 BM25 round-trip.
- `cargo xtask architecture` — confirms the new crate satisfies the production
  dependency allowlist (with the per-crate `rusqlite` exception), the source-
  tree unsafe ban, and the source-scanner forbidden-reference rules.
- `cargo xtask verify` — full workspace fmt + clippy `-D warnings` + tests +
  doc tests + architecture + provider/runtime/acp/sessions checks.

## Open questions (deferred to a future ADR)

1. **TUI command surface.** Whether cognitive memory gets new `/remember`
   `/recall` `/forget` slash commands or hooks into the existing `/memory`
   command. This ADR ratifies only the crate and the wiring; the command
   surface is additive and can ship independently.
2. **Snowball → real NLP.** Whether a future Rust NLP crate (or a Python
   sidecar) replaces the Snowball fallback to close the lemmatization +
   entity-extraction parity gaps.
3. **Reranker.** Whether a v2 ships with a reranker (Cohere/HF/etc.) once a
   provider-routed trait for reranking is established.
