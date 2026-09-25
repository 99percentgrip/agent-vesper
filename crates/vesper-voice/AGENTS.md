# Vesper Voice Core

## Purpose

Own the VRO-17 voice subsystem's pure core: validated PCM framing, STT/TTS
ports and descriptors, the error taxonomy, the speech-egress policy and
configuration contracts, the host↔core event vocabulary, per-turn report
types, and deterministic in-memory fakes.

## Ownership

- Pure core: contracts, compositions, and fakes — no I/O, no clock, no
  device access, no vendor names; workspace deps exactly
  `vesper-domain` + `vesper-security`.
- `execution` (clarified R16): the pure stage selection rule — per-stage
  CPU / Automatic-verified-only / strict-NPU policies, ten evidence-based
  `AcceleratorReadiness` facts (only `Ready` permits acceleration),
  in-process exact-identity readiness caching with invalidation events,
  and honest offload placement attribution (Cpu/Full/Hybrid/Unverified —
  a backend claim without per-request device evidence is never promoted).
  CPU policy is structurally acceleration-blind. No device, runtime, or
  model knowledge lives here; hosts supply facts from bounded passive
  inspection. Evidence: `docs/foundation/voice-capability-gated-execution.md`.
- Feature-gated adapters (default-off, zero production dependencies):
  `stt-sidecar` (Rust adapter driving the shipped Python sidecar —
  compatibility inference, truthfully labeled; embedded script is the
  single source of truth with the TUI dictation path; warm shared child
  with process-group-owned reaping; bounded line IO; setup-prerequisite
  unavailability) and `stt-http` (configured self-hosted worker; strict
  validated endpoint; redirects refused; D21 `LegacyEmptyResponse`
  provenance for legacy-worker empties). Future adapters (TTS, cloud)
  require per-PR dependency-rule evaluation; default-off features never
  grant dependency ownership.
- Compositions: `FailoverStt` (only `Unavailable` advances; per-attempt
  egress recheck; attempt bound), `PartialGate` (buffered-repass
  partials; whole-capture budget; final priority; stale rejection; no
  turn-submission surface exists), `run_blocking` (cancellable bridge;
  honest computation-continues limit documented in-module).
- `hygiene` (PR-2): the stateful bounded sentence gate — hygiene runs
  on gated segments; protected spans (reasoning/code/PEM) tracked
  across chunk and sentence boundaries; credential/blob redaction with
  truthful markers; overflow is loud and never flushes unvalidated
  text; finalize is exactly-once. Unit coordination is PR-3.
  VRO-17 continuity/formatting repair: presentation list markers
  (`N.` digits+period at a sentence start, ≤3 digits) keep their period
  from terminating a sentence — the marker stays attached to its item so
  `"1. Do the first thing"` is one unit (a bare `"1."` phonemizes to
  nothing and was the device failure). A drained unit that is ONLY a
  presentation prefix (bare marker, `#`-only delimiter, single bullet
  glyph, `|---|` rule) defers instead of emitting; finalize records it
  as `SkippedSpan { class: "formatting-only" }` — never an empty
  `SpeakUnit` or a model call. Meaning is never dropped: headings keep
  their word, `The answer is 1.`/decimals/versions never match the
  marker shape, and symbols-as-content still emit (visible engine
  failure if unspeakable). Known limitation: a chunk boundary splitting
  a decimal at `"N.|rest"` can terminate mid-decimal (pre-existing;
  needs general end-of-pending holdback).
- `session` (PR-3): the production `VoiceSession` reducer — host events
  in, typed `VoiceEffect`s out (exactly-one `SubmitTurn`, transactional
  `CancelRuntimeTurn`, urgent-ordered `StopPlayback`/`CancelSynthesis`);
  five concurrent dimensions with a derived phase projection; D21
  provenance gates submission; hygiene fed exactly once per turn;
  pre-identity cancellation intent applies to the matching late run;
  Stop is idempotent; pending input is bounded with visible rejection;
  playback receipts validated (identity/monotonic/gap) and interruption
  notes composed once from acknowledged sanitized text — and when no
  unit was acknowledged at all, the note claims missing evidence
  ("progress was not confirmed"), never absence of delivery; reports
  are metadata-only with absent stages absent. Hosts execute all
  effects; the session performs no I/O.
- `tts-subprocess` (PR-2, feature): **synthesis-only** — text in,
  canonical PCM out via the `VoiceTts` port; never plays audio, never
  creates files (stdout → memory), never treats process exit as
  playback. System-engine route is an optional user-installed baseline
  (no bundling; licenses recorded in the PR-2 execution report). Spawning a
  newly published executable retries only the transient `ExecutableFileBusy`
  classification twice with a 50 ms delay; other failures remain immediate.
- The crate must never depend on runtime, agent, harness, or provider
  adapters. Hosts translate existing runtime/provider events into the
  voice-owned `HostEvent` vocabulary at their composition boundary; the
  core never sees provider stream or runtime types.
- Cancellation is the locally owned `VoiceCancel` (waker-aware,
  hierarchical children). Hosts cancel it alongside — never instead of —
  the runtime's own cancellation path.

## Local Contracts

- Canonical audio: i16 LE, 16 kHz, mono. `PcmFrame` is validated and
  sample-aligned; only `PcmReassembler` builds frames from transport
  bytes, carrying odd bytes and reporting end-of-capture truncation.
- Error taxonomy separates `NoSpeech` (nonfatal, not failover-eligible)
  from `Unavailable` (failover-eligible); engine failures never become
  silence. TTS streams distinguish open failure, mid-stream failure,
  clean finish, and cancellation.
- Speech egress policy: `OnDevice` speech never reaches
  `SelfHostedRemote`/`ThirdPartyCloud`; cloud synthesis hygiene is
  structural (no schema switch exists to disable it).
- Reports and error strings carry metadata only — no audio bytes, no
  transcript text, anywhere in the taxonomy or telemetry surface.
- Fakes are visibly named `Fake*`, carry `"fixture": true` markers, and
  prove contracts only — never provider behavior.

## Work Guidance

- PRD: `docs/voice-oracle-extraction-prd.md` (decision record §7,
  traceability matrix §8). PR-0 evidence:
  `docs/foundation/voice-oracle-pr0-execution.md`.
- Reference upstream is the voice oracle (alias rule enforced by
  `cargo xtask naming-guard`); mechanisms only, never ported source.

## Verification

- `cargo test -p vesper-voice` and
  `cargo test -p vesper-voice --features stt-sidecar,stt-http,tts-subprocess`
  (unit + `tests/pr0_contracts.rs` + `tests/pr1_adapters.rs` +
  `tests/pr2_tts.rs` including the storage-safety suite). Tests whose fixtures
  are executable POSIX shell wrappers run only on Unix; Windows still compiles
  and runs the platform-neutral adapter validation, HTTP, composition, and
  contract cases.
- `cargo xtask architecture` enforces the dependency allowlist
  (vesper-domain, vesper-security only).
- Pure core must build with no features; adapter features must stay
  default-off with no `dep:` entries (asserted by the manifest test).

## Child DOX Index

No children.
