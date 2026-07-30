# Stage 5 Read-Only Persistence Evidence Index

Status: COMPLETE

## Scope

Read-only session persistence ports, platform-aware layouts, bounded discovery
and schema-v1 decoding, deterministic composite precedence and compatibility
identities, pure runtime-state conversion, ordered writer-acknowledged ACP
replay, runtime/ACP/composition injection, process disk-invariance evidence,
fixture/testkit governance, and final readiness. Persistent writes, SQLite,
repair, migration, and Stage 6 implementation are excluded.

## Preflight state

- Target: `/home/alex/Projects/Agent Vesper`
- Target branch/status: `main`; no commits; all existing project content is
  untracked and preserved.
- Frozen source: `/home/alex/Projects/Native GLM-5.2 Provider`
- Frozen source HEAD: `bf4d4287e2e3320aa3f09015f678e6169d520045`
- Frozen source status: zero tracked changes; only pre-existing
  `docs/codex-tui-roadmap-prompt.md` is untracked.
- Workspace before this part: 11 packages; no session persistence crate.
- Fixture corpus remains 76 scenarios / 154 indexed payloads.

## Confirmed source behavior

- `glm_acp/session_store.py:59-69`, `SessionStore.__init__`, selects
  `~/.glm-acp/sessions/` for the default profile and
  `~/.glm-acp/profiles/<profile>/sessions/` for a named profile.
- `glm_acp/session_store.py:71-78`, `SessionStore._path`, replaces every character
  outside `[a-zA-Z0-9_-]` with `_` and adds `.json`.
- `glm_acp/session_store.py:247-285`, `SessionStore.list`, treats a missing root as empty
  and enumerates direct metadata/session files without recursion.
- `tests/test_session_store.py` confirms traversal and slash-bearing IDs map to
  a filename inside the configured session root and missing roots list empty.

## Documentation evidence

- Context7 was consulted first for Tokio APIs but its configured quota was
  exhausted.
- Official Tokio documentation confirms `spawn_blocking` can queue work after
  its blocking-thread limit; Stage 5 therefore acquires an explicit semaphore
  permit before scheduling each bounded synchronous filesystem operation.

## Commands executed

- `git status --short --branch`, `git remote -v`, Cargo metadata/tree commands,
  fixture index inspection, source `git rev-parse`/status checks.
- Targeted reads/searches of applicable `AGENTS.md`, Stage 4/Stage 5 readiness
  records, workspace manifests, current session DTOs/config paths, xtask
  architecture rules, and frozen `session_store.py` plus focused tests.
- `cargo check -p vesper-sessions` — passed.
- `cargo test -p vesper-sessions` — passed: 11 tests, zero failures/ignored.
- `cargo fmt --all --check` — passed.
- `cargo clippy -p vesper-sessions --all-targets --all-features -- -D warnings`
  — passed.
- `cargo xtask architecture` — passed for 12 workspace packages.
- Dependency/forbidden-API scans and final target/source Git checks — passed.

## Files created or modified

- Created `crates/vesper-sessions` with contracts, layouts, filename policy,
  bounded filesystem source, composite source, and focused tests.
- Updated workspace membership, architecture enforcement, dependency ownership,
  workspace map, migration status, and applicable DOX records.

## Dependency changes

- Added no new third-party dependency. The crate reuses pinned `thiserror`
  2.0.19 and Tokio 1.52.0 plus workspace crates `vesper-domain` and
  `vesper-config`.
- No SQLite/FTS dependency was added.

## Read-only checks

- `cargo xtask architecture` rejects filesystem mutation APIs, SQLite, ACP,
  runtime, and concrete-provider references from production session sources.
- Tests confirm missing roots return empty/absent and remain uncreated.
- Tests confirm non-recursion, entry/filename/file-size limits, safe filename
  mapping, symlink escape refusal, and deterministic collision precedence.
- Frozen source final invariance: HEAD remains
  `bf4d4287e2e3320aa3f09015f678e6169d520045`; tracked diff is empty; only
  `docs/codex-tui-roadmap-prompt.md` remains untracked.

## Final target state

- Target remains on `main` with no commits and no remote changes.
- Git reports the same repository-wide untracked top-level content that
  predated this part, now including Stage 5 additions beneath `crates/` and
  `docs/`; no commit was created.
- No real Agent Vesper or Native GLM ACP state path was opened for mutation.

## Unresolved questions

- Non-Linux host behavior remains CI validation pending; layout behavior is
  tested using injected paths and is not presented as real-host validation.

## Part 2 — Legacy decoding and metadata

Status: COMPLETE

### Confirmed source behavior

- `glm_acp/agent.py:555-665`, `Session.to_dict` / `Session.from_dict`, defines
  schema version 1, compatibility defaults, GLM settings, messages, usage,
  plan, learning/verification envelopes, goals, tools, and lineage behavior.
- `glm_acp/session_store.py:209-229`, `SessionStore.save`, defines the legacy
  sidecar fields: session ID, cwd, title, update timestamp, parent, and branch
  root.
- `glm_acp/session_store.py:247-285`, `SessionStore.list`, consumes valid
  sidecars first, falls back to `.json` when a sidecar is absent/corrupt, skips
  corrupt entries, and sorts newest first.
- `glm_acp/agent.py:2008-2020`, `GlmAcpAgent.list_sessions`, filters cwd using
  exact string equality.
- `tests/test_session_store.py:119-161` confirms sidecar-first listing,
  fallback-compatible metadata, recency ordering, and fail-soft corrupt JSON.

### Files created or modified

- Added `src/decoder.rs` with `LegacySessionDecoder`, `LegacyDecodeBounds`,
  `LegacyLoadOutcome`, and typed corruption/bound classifications.
- Added `src/metadata.rs` with safe sidecar/JSON extraction and deterministic
  newest-first ordering.
- Extended `src/contracts.rs` metadata and exact cwd-filter contracts.
- Extended `src/filesystem.rs` with bounded `.meta` enumeration, sidecar-first
  fallback, and fail-soft unusable-entry handling.
- Added `tests/legacy_decode.rs` and `tests/metadata_listing.rs`; updated the
  Part 1 read-only tests for the richer metadata contract.
- Updated applicable DOX, architecture, workspace, dependency, and migration
  records.

### Enforced decode bounds

| Surface | Default bound |
| --- | ---: |
| Session file | 16 MiB |
| Messages | 10,000 |
| Individual content/reasoning value | 1 MiB |
| Plan items / measured plan data | 1,000 / 1 MiB |
| Additional roots / root bytes | 128 / 4,096 |
| Unknown top-level fields | 256 |
| Unknown measured data / nodes | 1 MiB / 100,000 |
| JSON compatibility depth | 64 |
| Lineage identifier | 256 bytes |
| Compatibility array | 1,000 items |
| Flexible compatibility envelopes | 4 MiB |
| Metadata sidecar | 64 KiB |
| Metadata fields | 64 |
| Metadata nodes / depth | 100,000 / 64 |
| Title / cwd | 1,024 / 4,096 bytes |
| Timestamp / model / provider | 128 / 256 / 64 bytes |

The filesystem byte limit is checked before reading. Parsed structures are
measured iteratively; no allocation capacity is taken from a declared JSON
length. Unknown schema-v1 fields remain in the Stage 2
`LegacySessionV1::unknown_fields` map for later explicit writer design.

### Typed outcomes

- `Loaded`
- `Missing`
- `Corrupt`
- `UnsupportedVersion`
- `RejectedByBounds`
- `PermissionDenied`
- `UnsafePath`

No outcome repairs, rewrites, migrates, deletes, or creates user state.

### Metadata behavior

- Valid bounded sidecars win even if the history JSON is corrupt.
- Missing, corrupt, oversized, mismatched, or unsafe sidecars fall back to
  bounded JSON inspection when usable JSON exists.
- Unusable entries are skipped without failing unrelated listing entries.
- Ordering is update timestamp descending with session ID ascending as the
  deterministic tie-breaker.
- Cwd filtering is exact string equality; no substring or implicit
  canonicalization is used.
- Listings expose no message bodies, system prompts, reasoning, tool internals,
  credentials, or inferred previews.

### Documentation evidence

- Context7 was attempted first for pinned `serde_json` behavior and reported its
  monthly quota exhausted. The API-key file was not read.
- Implementation was verified against the pinned crate, frozen source behavior,
  authoritative fixtures, and local tests.

### Verification

- `cargo check -p vesper-sessions` — passed.
- `cargo test -p vesper-sessions` — passed: 20 tests, zero failures/ignored.
- `cargo clippy -p vesper-sessions --all-targets --all-features -- -D warnings`
  — passed.
- `cargo fmt --all --check` — passed.
- `cargo xtask architecture` — passed for 12 workspace packages.
- Seven authoritative session scenarios have typed coverage: five complete
  fixture states decode, corrupt JSON classifies as corrupt, and the
  unknown-field contract round-trips without loss.

### Dependency changes

- Added the already pinned `serde_json` 1.0.151 as a direct
  `vesper-sessions` dependency.
- Added no new package version and no SQLite/FTS dependency.

### Final invariance

- Frozen source HEAD remains
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Frozen source tracked diff remains empty; only the pre-existing
  `docs/codex-tui-roadmap-prompt.md` is untracked.
- Target remains on `main` with no commits. Repository-wide pre-existing
  untracked content is preserved.
- Production session sources contain no filesystem mutation API, SQLite, ACP,
  runtime, or concrete-provider dependency.

### Deferred by the Part 2 stop condition

- Runtime conversion — completed in Part 3 below.
- Identity generation — completed in Part 3 below.
- Replay engine — completed in Part 3 below.
- Persistent writes, repair, migration, and search

## Part 3 — Runtime state, identity, and replay

Status: COMPLETE

### Confirmed source behavior

- `glm_acp/agent.py:1941-2000`, `GlmAcpAgent.load_session`, restores the record,
  recalculates transient token estimates, then awaits history replay, plan
  replay, and available-command replay before returning the lifecycle response.
- `glm_acp/agent.py:2031-2079`, `GlmAcpAgent.resume_session`, uses the same
  ordered replay behavior.
- `glm_acp/agent.py:6790-6823`, `_replay_history`, emits only non-empty
  user/assistant content, flattens supported multipart text blocks, and skips
  system messages, tool results, and internal entries.
- `tests/test_agent.py:2288-2345`, `TestReplayHistory`, confirms system/tool/
  empty suppression and multipart text replay.
- `glm_acp/agent.py:4748-4766`, `_send_plan`, maps persisted plan entries to an
  ACP plan update without executing them.
- `glm_acp/agent.py:5094-5246`, `_send_available_commands`, emits the command
  catalog before load/resume returns. Stage 5 truthfully emits an empty catalog
  because the minimal runtime implements no slash commands.

### Files created or modified

- Added `crates/vesper-sessions/src/conversion.rs` for pure decoded-record
  conversion, compatibility availability checks, active history filtering,
  tool-call/result pairing, cumulative usage, and redacted compatibility data.
- Added `crates/vesper-sessions/src/replay.rs` for ACP-neutral ordered replay
  plans and an acknowledgement-based `ReplaySink`.
- Added `crates/vesper-sessions/tests/runtime_conversion.rs` with conversion,
  identity, unavailable-configuration, and replay-barrier tests.
- Extended `vesper-runtime::SessionSnapshot` with converted source, endpoint,
  configuration status, compatibility, and replay state; added
  `SessionSnapshot::from_persisted`.
- Added `crates/vesper-runtime/tests/persisted_state.rs`.
- Extended `vesper-acp` load/resume replay mapping for visible messages, plan,
  session metadata/mode, and available commands using the existing physical
  writer-acceptance gate.
- Updated workspace architecture enforcement, dependency records, migration/
  architecture/workspace documentation, README, and the applicable DOX chain.

### Conversion rules

- Preserved runtime fields: session/source identity, cwd and bounded additional
  roots, provider-qualified model, endpoint reference, provider configuration
  envelope, lineage, modes, valid active history, cumulative usage, and the
  complete frozen compatibility record.
- System records and orphan tool results do not enter active history. Valid
  user/assistant messages and structurally paired tool calls/results do.
- Provider continuation reasoning is retained as redacted provider-opaque
  compatibility content and is never replayed as visible text.
- Persisted plan/goal/tool/memory/checkpoint state is never executed.
- Unknown or unavailable provider/model/endpoint values remain inspectable and
  replayable with `SessionConfigurationStatus::ConfigurationRequired`; the
  session actor refuses provider dispatch until configuration is resolved.

### Deterministic identity

- Existing valid legacy message IDs are preserved.
- Missing IDs use SHA-256 with the domain separator
  `agent-vesper-legacy-identity-v1`, session identity, original message ordinal,
  and role.
- IDs are stable across repeated loads, role-distinct, bounded, and contain no
  message-content hash. The legacy record is never rewritten.

### Replay barrier

- `ReplayPlan` fixes the sequence to visible messages, optional plan, metadata/
  mode, and available commands.
- `ReplayPlan::deliver` awaits each sink acknowledgement sequentially.
- The ACP sink resolves acknowledgement only after the official SDK update is
  observed at the physical stdout writer gate. Load/resume responses are sent
  only after `deliver` returns.
- Updates are mapped one at a time and encoded by the SDK; the complete encoded
  transcript is not materialized or duplicated in memory.

### Commands and verification

- Re-read all applicable root/crates/sessions/runtime/ACP/docs Stage 5 DOX.
- Inspected targeted domain content/tool/usage/version contracts, runtime actor
  and snapshot ownership, ACP SDK replay/writer paths, pinned SDK schema types,
  frozen load/resume/history/plan/command symbols, and focused source tests.
- Context7 was attempted first for the pinned SHA-2 API and reported its
  monthly quota exhausted; no API key or credential file was read.
- `cargo check -p vesper-sessions` — passed.
- `cargo test -p vesper-sessions` — passed: 24 tests, zero failures/ignored.
- `cargo test -p vesper-runtime -p vesper-acp` — passed: 8 tests, zero
  failures/ignored.
- `cargo clippy -p vesper-sessions -p vesper-runtime -p vesper-acp
  --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --all --check` — passed after formatting.
- `cargo xtask architecture` — passed for 12 packages.
- Forbidden SQLite/write API scan across sessions/runtime/ACP — clean.

### Dependency changes

- `vesper-sessions` now directly uses already-pinned `sha2` 0.11.0 for
  compatibility IDs; no package version was added.
- `vesper-runtime` now depends on `vesper-sessions` pure converted state. The
  reverse dependency remains forbidden, so the graph is acyclic.
- No SQLite/FTS, ACP SDK in sessions/runtime, concrete provider, or filesystem
  mutation dependency was added.

### Final invariance and stop boundary

- Frozen source HEAD remains
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Frozen source tracked diff remains empty; only pre-existing
  `docs/codex-tui-roadmap-prompt.md` is untracked.
- Target remains on `main` with no commits; all pre-existing untracked work is
  preserved.
- No runtime session-repository injection, composition-binary configuration,
  or persistence process transcript was implemented.
- No Agent Vesper or Native GLM ACP session state was written.

## Part 4 — Runtime, ACP, and composition read integration

Status: COMPLETE

### Files and contracts

- Added `crates/vesper-sessions/src/vesper_format.rs` with
  `VesperSessionV1`, `VesperSessionDecoder`, strict version/discriminator
  checks, bounded collection validation, approved namespaced extensions, and
  typed read outcomes. No writer API exists.
- Added `EmptySessionRepository` for explicitly disabled sources and retained
  composite precedence: current in-memory actor, Agent Vesper read store, then
  legacy read store.
- Added `crates/vesper-runtime/src/persistence.rs` with
  `RuntimeSessionReads`. The supervisor checks actors first, reads/decode/
  converts on misses, adopts state into one actor, and creates only an
  ephemeral session when all read sources miss.
- Runtime listing merges actors and persistent metadata with in-memory IDs
  winning. Load/resume validate the caller primary workspace, fork stays
  in-memory, and close removes only the actor.
- `HarnessCommandPayload::{LoadSession,ResumeSession}` now carry complete
  workspace-root context instead of losing ACP `cwd`.
- `vesper-acp` maps persistent read outcomes to bounded stable error reasons,
  forwards complete workspace roots, and retains the existing physical-writer
  replay acknowledgement barrier before lifecycle responses.
- `agent-vesper-acp` supports opt-in Agent Vesper and legacy readers, explicit
  injected roots, optional legacy profile, `max_session_bytes`, and
  `max_entries`. Missing roots are not created; relative roots fail closed.
- Updated `xtask` dependency allowlists, architectural records, workspace map,
  dependency register, migration status, README, and applicable DOX.

### Read-only configuration

- `AGENT_VESPER_ENABLE_SESSION_READS` (or the explicit alias
  `AGENT_VESPER_ENABLE_VESPER_SESSION_READS`) enables the Agent Vesper data
  reader.
- `AGENT_VESPER_ENABLE_LEGACY_SESSION_READS` enables legacy discovery.
- `AGENT_VESPER_SESSION_ROOT` and
  `AGENT_VESPER_LEGACY_SESSION_ROOT` inject absolute test/store roots.
- `AGENT_VESPER_LEGACY_PROFILE` selects the confirmed named-profile layout.
- `AGENT_VESPER_SESSION_MAX_BYTES` and
  `AGENT_VESPER_SESSION_MAX_ENTRIES` override positive bounded defaults.
- Startup configuration constructs descriptors/readers only. It creates no
  directory, session record, index, sidecar, or state file.

### Tests and commands

- Inspected target status/dependency graph, Stage 2 compatibility DTOs,
  Stage 5 Parts 1–3 code/evidence, runtime actor lifecycle, ACP load/resume
  mappings, composition startup, and focused frozen-source session behavior.
- `cargo check -p vesper-sessions -p vesper-runtime` — passed.
- `cargo check -p vesper-acp` — passed.
- `cargo check --workspace --all-targets --all-features` — passed.
- `cargo test -p vesper-sessions` — passed: 27 tests after Part 4 additions.
- `cargo test -p vesper-runtime` — passed: 6 tests, including synthetic
  filesystem list/load/cache/fork/close and missing-ID ephemeral fallback.
- `cargo test -p vesper-acp` — passed: 4 tests.
- `cargo test -p agent-vesper-acp --lib --bins` — passed: 2 tests; existing
  process transcript integration suites were intentionally not invoked by the
  Part 4 stop condition.
- `cargo fmt --all --check` — passed.
- `cargo clippy -p vesper-sessions -p vesper-runtime -p vesper-acp -p
  agent-vesper-acp --all-targets --all-features -- -D warnings` — passed.
- `cargo xtask architecture` initially detected the newly intentional app
  dependencies; its allowlist was updated to the accepted composition
  boundary and the final check passed.

### Dependency and safety result

- Added only the already locked `serde` dependency directly to
  `vesper-sessions`; no new package version entered `Cargo.lock`.
- Added no SQLite, FTS5, database, write, repair, delete, migration, search, or
  process-transcript implementation.
- Tests write only synthetic records below temporary directories and remove
  them. Production paths expose read-only ports only.

### Invariance and deferred boundary

- Frozen source HEAD remains
  `bf4d4287e2e3320aa3f09015f678e6169d520045`; tracked diff is empty and the
  pre-existing roadmap document remains the only untracked source file.
- Target remains on `main` with no commits; all pre-existing work is preserved.
- Security-invariant expansion and persistence process-level transcript tests
  are deferred exactly as required by the Part 4 stop condition.

## Part 5 — Security, consistency, and process disk invariance

Status: COMPLETE

### Production corrections

- `vesper-runtime::RuntimeSupervisor` now maintains short-lived keyed load
  gates. Loads for one session ID serialize through that ID only; different
  IDs retain concurrent repository access. The gate registry does not own
  session state and releases entries after the last waiter.
- Persistent adoption rechecks the in-memory registry after the read. A newer
  actor therefore wins over a stale completed disk read.
- `vesper-domain::ExtensionMap` now rejects secret-shaped namespaced top-level
  keys as well as nested secret keys. A future Agent Vesper record containing
  `provider:api-key` is rejected before runtime adoption.
- `SessionStoreError::RootNotAbsolute` no longer stores or renders the private
  path. ACP persistent errors remain bounded stable reason codes.
- The pre-existing missing-session process expectation was corrected to the
  confirmed source-compatible behavior: load creates only an ephemeral actor
  when all read sources miss.

### Adversarial security evidence

| Invariant | Evidence |
| --- | --- |
| ID/path containment and traversal rejection | `read_only::requested_id_is_mapped_and_never_used_as_a_path` |
| Symlink escape rejection | `read_only::symlink_escape_is_rejected` |
| Preallocation file-size bound | `read_only::entry_filename_and_record_bounds_fail_closed` |
| JSON depth/aggregate/unknown bounds | `legacy_decode::version_and_each_high_risk_bound_have_typed_outcomes` |
| Private-path error redaction | `read_only::errors_do_not_render_private_root_paths` |
| Listing excludes reasoning/system/tool data | `metadata_listing` fallback vectors |
| Raw provider key rejection | `vesper_format::future_format_rejects_versions_bounds_and_identity_mismatch` and process `raw-secret` vector |
| Visible-only replay | process `legacy-minimal-load-and-safe-replay` vector |
| Corrupt record not repaired | process `corrupt` vector plus exact disk snapshot |
| Permission denial is typed/fail-safe | `legacy_decode::missing_permission_and_unsafe_paths_are_distinct` |

### Concurrency and replacement evidence

- Concurrent load and resume of the same persisted ID return one actor; final
  listing contains the ID once.
- A different session load and listing execute concurrently with the same-ID
  operations. Filesystem blocking work remains bounded by the existing
  semaphore rather than a global filesystem lock.
- `completed_persistent_read_cannot_overwrite_a_newer_in_memory_actor` delays a
  real synthetic filesystem reader, creates a newer actor, then releases the
  read and confirms the in-memory actor wins.
- `atomic_replacement_during_read_is_consistent_and_typed` performs 100 atomic
  replacements and 100 bounded reads; every result is loaded, corrupt, or
  missing and no repair occurs.

### Real-process transcript and disk proof

- Added eleven independent persistence process executions to the existing
  `process_blockers` real-binary suite:
  listing; legacy minimal load/safe replay; resume; unknown fields; missing
  metadata; fork; close; cross-source collision; corrupt record; unsupported
  version; and raw-secret rejection.
- Every execution traverses `agent-vesper-acp` → `vesper-acp` →
  `vesper-runtime` → `vesper-sessions` with explicit synthetic roots.
- Each vector hashes nine files before and after, records byte length and
  modification time, and compares the complete root-qualified file map.
- Result: 99 exact before/after file-state comparisons, 198 SHA-256
  computations, zero changed hashes, zero changed lengths, zero timestamp
  changes, and zero added/removed files.
- The process created none of its isolated XDG config/cache/data/state
  directories. Secret, system-prompt, and reasoning canaries were absent from
  stdout and stderr.
- Detailed evidence is in `docs/stage5/disk-invariance-proof.md`.

### Commands and results

- `cargo check -p vesper-domain -p vesper-sessions -p vesper-runtime -p
  agent-vesper-acp --all-targets --all-features` — passed.
- `cargo test -p vesper-domain -p vesper-sessions -p vesper-runtime` — passed:
  17 domain, 29 sessions, and 8 runtime tests in the final corpus.
- Focused persistence process run — passed: 3 test functions driving 11 real
  processes.
- Complete `process_blockers` run — passed: 11 tests (8 Stage 4.1 plus 3
  persistence drivers).
- The first complete app test run found one stale Stage 4 expectation that a
  missing load should error. After correcting it to the accepted ephemeral
  fallback contract, `cargo test -p agent-vesper-acp --tests --all-features`
  passed: 2 app unit tests, 11 blocker/process tests, and 4 transcript tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo fmt --all --check` — passed in the final verification pass.
- Architecture-enforcement expansion and final-stage documentation were not
  started, per the Part 5 stop condition.

### Dependency and platform classification

- Added no package version. The ACP app uses the already locked `sha2` 0.11.0
  only as a dev dependency for synthetic file hashing.
- Linux x86-64 process behavior is locally validated. Other supported target
  families remain CI-validation pending.

### Invariance

- Frozen source HEAD remains
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Frozen source tracked diff is empty; only the pre-existing
  `docs/codex-tui-roadmap-prompt.md` remains untracked.
- No real Agent Vesper or Native GLM ACP state was opened or modified.

## Part 6 — Governance, coverage, and final verification

Status: COMPLETE — CI VALIDATION PENDING

### Files created or modified

- Created `fixtures/coverage-stage5.json`.
- Added `crates/vesper-testkit/src/session_store.rs` and public exports for
  temporary legacy/Agent Vesper read-store builders, session fixture loading,
  corrupt/truncated records, complete tree hashing, and no-write assertions.
- Added `cargo xtask sessions verify` and Stage 5 coverage generation/
  validation.
- Extended architecture enforcement for SQLite dependencies, filesystem
  mutation/directory creation, writer-shaped session APIs, and production
  fixture-scenario ID branches.
- Added Stage 5 workspace metadata and Cargo Deny bans for `rusqlite`, `sqlx`,
  and `libsqlite3-sys`.
- Updated CI/MSRV/five-target workflows and root architecture, dependency,
  workspace, migration, README, and DOX records.
- Created `session-store-report.md`, `legacy-discovery.md`,
  `runtime-load-and-resume.md`, `replay-contract.md`, and
  `stage6-readiness.md`.

### Fixture result

- Corpus: 76 scenarios, comprising 65 source-captured scenarios and 11
  synthetic future-contract vectors.
- Index: 154 payloads.
- Index SHA-256:
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- `rebuild-index` was run twice. The two canonical outputs and the repository
  index were byte-identical and had the same SHA-256.
- Stage 5 coverage represents 17 applicable contract surfaces, all seven
  source session scenarios, 14 applicable process-transcript mappings, and
  explicitly records zero persistent writes. Every other scenario names its
  future owner.

### Testkit result

- `SessionFixtureLoader` loads exactly the seven session fixtures.
- `LegacyStoreBuilder` supports default/named-profile synthetic stores,
  sidecars, corrupt records, and truncation.
- `AgentVesperReadStoreBuilder` supports synthetic future-format records,
  corruption, and truncation.
- `FileTreeHashManifest` records files, directories, and symlinks without
  following symlinks.
- `NoWriteAssertion` rejects any file-set/content/structure difference.
- `cargo test -p vesper-testkit` passed 23 tests.
- No production crate depends on `vesper-testkit`.

### Commands and results

- `cargo check --workspace --all-targets --all-features` — passed.
- `cargo fmt --all --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo test --workspace --all-features` — passed: 151 tests, zero failures
  or ignored tests.
- `cargo test --workspace --doc` — passed.
- `cargo xtask fixtures validate` — 76 scenarios.
- `cargo xtask fixtures verify-index` — 154 payload hashes.
- `cargo xtask fixtures coverage --stage 5` — passed; seven sessions and no
  writes.
- `cargo xtask contracts verify` — passed.
- `cargo xtask provider glm verify` — passed.
- `cargo xtask runtime verify` — passed.
- `cargo xtask acp verify` — passed, including both real-process suites.
- `cargo xtask sessions verify` — passed.
- `cargo xtask architecture` — passed for 12 packages.
- `cargo xtask verify` — passed the aggregate local suite.
- `cargo xtask msrv` — the same 151 tests passed on Rust 1.88.
- `cargo audit` — no vulnerability failure.
- `cargo deny --all-features check` — advisories, bans, licenses, and sources
  passed; the existing `getrandom`, `r-efi`, and `syn` duplicate-version
  warnings remain reviewed and documented.
- All four GitHub workflow YAML files parsed locally.
- Forbidden SQLite, ignored/TODO/unimplemented macro, broad-allow, and
  production scenario-ID scans passed.

### Stability result

- Same-ID concurrent runtime load: 10/10 independent repetitions passed.
- Atomic replacement/read consistency: 10/10 independent repetitions passed;
  each test performs 100 replacements and reads.
- Real-process persistence vectors: 5/5 complete repetitions passed; each
  repetition drives three test functions and eleven production processes.
- There were zero flaky failures and no test-runner retry was used.

### Documentation evidence

- The Context7-first workflow was used for Cargo Deny configuration. Context7
  reported its monthly quota exhausted and offered no per-call key input.
- The installed pinned Cargo Deny 0.20.2 template confirmed that `[bans].deny`
  accepts reasoned `{ crate = "...", reason = "..." }` entries.
- The real Cargo Deny command validated the resulting policy. The user-provided
  API-key file was not read.

### Local versus external proof

- Linux x86-64: locally validated.
- Linux ARM64, macOS Intel, macOS Apple Silicon, Windows x86-64: workflow-ready
  but remote-CI validation pending.
- No pending platform is described as passing.

### Invariance and target state

- Frozen source HEAD remains
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Frozen source tracked diff remains empty; only the pre-existing
  `docs/codex-tui-roadmap-prompt.md` remains untracked.
- No real Agent Vesper or Native GLM ACP state was opened for mutation.
- Target remains on `main`, has no commits and no remote change; all repository
  content remains untracked as pre-existing work plus the Stage 5 additions.
- No Stage 6 implementation was started.

### Readiness

The exact next bounded target is:

`Stage 6 — transactional Agent Vesper session writes, revisions, crash safety, and derived metadata`

Local Stage 5 gates pass. Remote four-target CI validation remains pending.
