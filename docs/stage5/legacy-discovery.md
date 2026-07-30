# Legacy Native GLM ACP Discovery

Status: COMPLETE

## Confirmed frozen behavior

- `glm_acp/session_store.py:59-69`, `SessionStore.__init__`, selects
  `~/.glm-acp/sessions/` for the default profile and
  `~/.glm-acp/profiles/<profile>/sessions/` for a named profile.
- `glm_acp/session_store.py:71-78`, `SessionStore._path`, replaces characters
  outside `[a-zA-Z0-9_-]` with `_` and appends `.json`.
- `glm_acp/session_store.py:247-285`, `SessionStore.list`, reads direct session
  and metadata files, treats a missing root as empty, falls back from unusable
  sidecars to bounded JSON inspection, skips unusable entries, and sorts by
  recency.
- `glm_acp/agent.py:2008-2020`, `GlmAcpAgent.list_sessions`, applies exact cwd
  equality rather than substring matching.
- `tests/test_session_store.py:119-161` confirms sidecar-first fail-soft
  listing, missing-root behavior, and deterministic newest-first results.

## Rust layout contract

`LegacySessionLayout` is descriptive only:

- default: `<home>/.glm-acp/sessions`
- named profile: `<home>/.glm-acp/profiles/<validated-profile>/sessions`

Layout construction and application startup create no directory. A missing
root lists empty and loads missing. Production code never opens real legacy
state in tests; all filesystem tests use synthetic temporary roots.

## Containment and enumeration

`FilesystemSessionStore`:

1. validates the configured root is absolute;
2. maps external IDs to safe direct-child filenames;
3. enumerates only the configured directory;
4. rejects excessive entries and filename length;
5. uses `symlink_metadata` and canonical containment checks;
6. checks metadata size before reading bytes;
7. executes blocking filesystem work only after acquiring a bounded permit.

No recursive discovery, repair, deletion, rename, metadata creation, or
sidecar generation exists.

## Metadata policy

Valid bounded `.meta` files are preferred. Missing, malformed, mismatched, or
oversized sidecars fall back to safe JSON metadata. Listings expose only
session identity, origin, title, cwd, update timestamp, provider/model,
lineage, optional explicitly safe preview, and read-only state. Reasoning,
system prompts, tool internals, provider secrets, and message bodies are never
listing fields.

Ordering is update timestamp descending with session ID ascending as the
stable tie-breaker. Cwd filtering is exact string equality.

## Compatibility and migration

Legacy discovery is read-only and opt-in at composition. It does not silently
move, rewrite, repair, or delete `~/.glm-acp`. Agent Vesper and legacy records
may coexist; deterministic composite precedence prevents ambiguous collision
selection.

