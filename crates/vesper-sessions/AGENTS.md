# Sessions

## Purpose

Own read-only session repository ports, bounded legacy compatibility decoding,
store layouts, safe filename mapping, metadata discovery, pure runtime-state
conversion, deterministic identities, replay plans, and the Stage 6
transactional Agent Vesper session writer with crash-safe atomic writes and
derived `.meta` sidecars.

## Ownership

- `src/contracts.rs` owns source, capability, metadata, record, reader, writer,
  and lister contracts.
- `src/layout.rs` owns descriptive Agent Vesper and legacy session roots.
- `src/filename.rs` owns source-compatible safe filename mapping.
- `src/filesystem.rs` owns bounded, non-recursive read isolation.
- `src/composite.rs` owns deterministic source precedence.
- `src/decoder.rs` owns typed schema-v1 load outcomes and compatibility bounds.
- `src/metadata.rs` owns safe sidecar/JSON listing extraction and ordering.
- `src/conversion.rs` owns pure compatibility state conversion, history
  filtering, and deterministic identity generation.
- `src/replay.rs` owns ACP-neutral update ordering and acceptance sinks.
- `src/search.rs` owns bounded read-only persisted user/assistant history
  search over repository ports; it uses no writable index and never includes
  reasoning, tool payloads, or secrets.
- `src/vesper_format.rs` owns the versioned Agent Vesper format and decoder.
- `src/writer.rs` owns the transactional Agent Vesper writer: write-to-temp,
  fsync, atomic rename, derived sidecar generation, per-session write
  isolation, orphan-temp sweep, and write bounds.

## Local Contracts

- Read-path modules remain strictly read-only: no filesystem writes, no
  directory creation, no mutation APIs. The `cargo xtask architecture` source
  scan enforces this for every module except `writer.rs`.
- `writer.rs` is the only module permitted to perform filesystem mutation. It
  creates only its configured absolute root (non-recursively), writes a sibling
  temp file inside the canonical root, fsyncs, and atomically renames it over
  the target so the rename never crosses a filesystem boundary on POSIX.
- The authoritative session record is committed before its derived sidecar; a
  crash between the two leaves a valid session whose metadata the reader
  regenerates from the JSON body.
- Atomicity, path containment, safe filename mapping, and configured byte
  bounds are enforced on every write. Orphaned temp files from a prior crash
  are swept on the next write of the same session ID.
- Writes to distinct session IDs run concurrently; writes to the same session
  ID are serialized by a per-ID mutex. Writes share only a bounded blocking
  semaphore.
- Legacy and in-memory stores remain read-only; the writer only targets the
  Agent Vesper source root.
- Conversion and decoding remain pure; runtime injects these read-only ports
  and the writer port without moving filesystem logic into the runtime.
- Replay plans execute no persisted plan, goal, tool, memory, or checkpoint.
- Persistent search is supported as a bounded linear scan over the existing
  session records. SQLite/FTS indexes remain intentionally absent; the scan
  is bounded and deterministic so it cannot recreate the oracle's WAL/FD leak.
- Generated IDs hash only session identity, ordinal, and role; they never
  rewrite legacy records or expose content hashes.

## Work Guidance

- Preserve unknown schema-v1 fields needed by a later safe writer.
- Reject unknown top-level Agent Vesper format fields; preserve approved
  namespaced extension data in the versioned extension envelope.
- Retain unsupported/internal history only as redacted compatibility data.
- Replay only non-empty user/assistant text and await sink acceptance.
- Never expose message bodies, reasoning, tool internals, or secrets in listing
  metadata.
- Treat missing roots as empty for reads; the writer creates its configured root
  on first write only when the parent already exists.
- Never use an external session ID directly as a path.
- Errors expose classifications and configured bounds, never private roots.
- Provider configuration extension maps reject raw secret-shaped keys.
- Plan entries are not yet retained across the runtime snapshot boundary
  (Stage 6 limitation) and are persisted as empty; the read path rebuilds
  replay from history.

## Verification

- Run `cargo check -p vesper-sessions`.
- Run `cargo test -p vesper-sessions`.
- Run `cargo xtask sessions verify`.
- Run `cargo xtask architecture`.

## Child DOX Index
