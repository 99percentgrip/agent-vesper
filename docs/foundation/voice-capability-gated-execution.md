# Capability-gated voice execution: stage policies, CPU preservation, and honest no-NPU behavior

> **Scope:** VRO-17 continuation after the latency/performance candidate repair.
> This unit implements the clarified R16 direction — stage-specific,
> evidence-based capability gating with CPU voice preserved on every machine —
> and proves no-NPU behavior through the real application path. It is **not**
> an NPU implementation: no NPU STT/TTS route is registered, advertised, or
> offered, because none exists yet (see the
> [first-speech/NPU assessment](voice-first-speech-and-npu-assessment.md)).

## Objective

Implement Alex's binding clarification for R16: never assign speech work to an
NPU merely because the machine or adapter exists; on machines without a
compatible usable path, ordinary CPU voice stays available without NPU
installation, NPU-absence warnings framed as voice failures, or any
acceleration requirement. Preserve the improved CPU candidate (smaller first
pieces, reusable Preview worker, F5/F9 semantics, shared STT, hygiene,
transactional cancellation, storage controls) unchanged. Advance the
CPU-acceptance and selection work without inventing support, without claiming
device success, and without advancing PR-5.

## User decision (binding, recorded)

From the continuation directive, verbatim intent:

```text
"never assign speech work to an NPU merely because Vesper supports an NPU
adapter or because Alex's laptop has one. On a machine without a compatible,
usable NPU path, ordinary CPU voice must remain available without NPU
installation, warnings presented as voice failures, or an acceleration
requirement."
```

This makes R16's acceleration support **conditional per machine, platform,
backend, model, and stage**; it does not remove the requested
supported-hardware work. The NPU STT/TTS gates in the PRD remain open.

## What was implemented

### 1. Pure-core stage execution policy (`crates/vesper-voice/src/execution.rs`)

New pure module (no I/O, no clock, no vendor names; same class as the rest of
the core; `cargo xtask architecture` enforces the unchanged dependency
allowlist):

- **`SpeechStage`** (`Stt` / `Tts`) — stages are independent; no shared
  `npu_available` boolean exists anywhere in the design.
- **`StageExecutionPolicy`** — `Cpu` (default), `AutomaticAccelerator`,
  `NpuRequired`, persisted as `stt_compute`/`tts_compute`
  (`cpu`|`automatic`|`npu`) in the `[voice]` scope. Absent keys default to
  `cpu`, so every pre-policy saved scope keeps today's CPU pipeline
  byte-for-byte (pinned by test).
- **`AcceleratorReadiness`** — ten distinct evidence-based facts
  (`NoRouteRegistered`, `Unsupported`, `DeviceAbsent`, `DetectionUnknown`,
  `SetupRequired`, `ModelUnsupported`, `AssetsMissing`,
  `VerificationPending`, `Unavailable`, `Ready`). `Ready` is the only
  acceleration-permitting state; a label, device file, GPU, vendor name,
  TOPS figure, or a previous machine's result never reaches it.
- **`resolve_stage`** — the single selection rule:
  - CPU → CPU, never consulting readiness (structurally
    acceleration-blind);
  - Automatic → the verified route when `Ready`, else the compatible CPU
    route with the actual reason exposed — an ordinary outcome, never a
    warning;
  - NPU-required → the verified route, else a stage-specific refusal that
    names the exact blocker and the Settings action; CPU is never reported
    as satisfying the request and the saved policy is never silently
    rewritten.
- **`ReadinessCache` / `ReadinessCacheKey` / `ReadinessInvalidation`** —
  in-process only, exact-identity lookup (machine+stage+backend+model+
  configuration), cleared on setup change/removal, backend failure,
  environment change, device loss, or resume. Readiness is never persisted,
  so copied configurations can carry intent but never stale success.
- **`OffloadPlacement` / `attribute_placement`** — honest post-execution
  attribution: `Cpu`, `Full` (per-request device evidence), `Hybrid`
  (partial graph), or `Unverified` (backend claim without per-request
  evidence — recorded, never promoted to a success claim).

### 2. Host seam (`apps/agent-vesper-tui/src/voice_accel.rs`)

- **`registered_routes()`** returns an **honest empty registry**: no real NPU
  speech adapter exists, so nothing is advertised, no setup is offered, and
  the strict-NPU option is hidden rather than decorative. The FLM-Whisper
  lead stays a documented gate, not today's implementation.
- **`stage_readiness`** — bounded passive inspection only (registry +
  existence checks); no vendor tool execution, no accelerator library
  loading, no device I/O. `route_readiness` orders device → runtime →
  assets/verification and never collapses a lesser fact upward.
- **`stage_route_for_policy`** — the production selection used by F9 and
  Settings: CPU policy returns without consulting the registry (zero
  accelerator calls anywhere).
- **`execution_rows` / `machine_capability_lines`** — the same shared
  assessment rendered for Settings ("Speech synthesis acceleration: using
  CPU — no compatible speech accelerator is registered in this build");
  no installer language for absent hardware.

### 3. Production wiring (F9 gate, Settings, save path)

- **F9 gate (`main.rs::conversation_gate_decision`)** now resolves both stage
  policies *before* prerequisites: a strict-NPU refusal blocks with the
  stage-specific message (stale copied preferences are explained and
  revalidated locally, never dispatched blindly and never rewritten);
  automatic-on-no-hardware resolves CPU and proceeds normally.
- **Settings → Voice (`settings_host.rs`)** gains two draft rows —
  "Speech recognition compute" and "Speech synthesis compute" — each opening
  a CPU / Automatic submenu (NPU-required appears only when a route is
  registered for that stage, satisfying the no-decorative-controls rule).
  The screen text states that CPU stays fully usable and that Automatic
  choosing CPU is an ordinary outcome. The **Readiness** panel appends the
  two machine-capability lines.
- **Save path (`settings_voice_save.rs`)** persists both keys; independent
  Save/dirty semantics and voice-only-change detection are unchanged.
- **Engine, voice, provider, reasoning choices are untouched**: no silent
  migration of the improved candidate; `EngineSelection::from_scope` is
  unchanged; STT/TTS provider selection stays separate from execution
  policy.

### 4. Regression suite (`apps/agent-vesper-tui/tests/voice_execution_policy.rs`)

Ten production-path tests (all against the real `resolve_stage`,
`stage_route_for_policy`, `execution_rows`, `save_voice_scope`,
`read_voice_scope`, and an instrumented registry fixture standing in for a
hypothetical registered route):

| Directive case | Test |
|---|---|
| No NPU + no runtime: CPU initializes; no attempt; no voice-disabled error | `no_npu_machine_cpu_initializes_and_no_acceleration_is_attempted` |
| CPU policy on fully capable fixture: zero accelerator calls | `cpu_policy_makes_zero_accelerator_calls_even_when_a_verified_route_exists` |
| Present but unsupported: Automatic=CPU; strict names exact blocker | `unsupported_machine_automatic_uses_cpu_strict_names_the_blocker` |
| Detection denied/timeout/unknown: no optimistic dispatch | `detection_unknown_is_never_positive_eligibility` |
| STT ready + TTS unsupported (and converse) | `stages_resolve_independently_no_shared_boolean` |
| Copied strict config: local revalidation, no rewrite, explanation | `copied_strict_config_is_locally_revalidated_and_explained_not_rewritten` |
| Busy/lost/runtime change: cache invalidation, bounded re-derivation | `readiness_cache_invalidates_on_events_and_never_crosses_machines` |
| Backend claims NPU but runs CPU/partial graph | `placement_attribution_never_fabricates_full_npu_success` |
| Real Settings save/read round trip + shared rows | `settings_save_and_execution_rows_round_trip_both_policies` |
| Unconfigured scope/build honesty | `unconfigured_scope_stays_fully_disabled_and_defaults_are_cpu` |

Plus pure-core unit tests in `execution.rs` (8 tests: CPU-blind resolution,
automatic-verified-only, strict refusal matrix, independent stages, cache
identity/invalidation, placement honesty, token round-trip) and two new
config tests (defaults preserve CPU; independent malformed-value errors).

**Red-first honesty note:** the suite was written against the clarified
policy; on the pre-change tree the policy rows, gate resolution, and save
keys did not exist, so the Settings/gate/save tests fail by construction
(cannot compile/parse) on the old code. The pure selection-rule tests
exercise semantics that had no prior implementation to violate. The one
behavioral red observed during development — the strict-refusal message
using the lowercase stage label while Settings rows use title case — was
fixed in the core (`SpeechStage::title`) rather than by weakening the
assertion.

## Methods and commands

```sh
cargo test -p vesper-voice --lib                          # 85 passed
cargo test -p vesper-voice                                # 85+13+29
cargo test -p vesper-voice --features stt-sidecar,stt-http,tts-subprocess  # +30+19
cargo test -p agent-vesper-tui --features voice-kokoro --lib   # 278 passed
cargo test -p agent-vesper-tui --lib                      # 242 passed (default build, R9 parity preserved)
cargo test -p agent-vesper-tui --features voice-kokoro \
  --test voice_execution_policy                           # 10 passed
cargo test -p agent-vesper-tui --features voice-kokoro \
  --test pr4_f9_gate --test pr4_wiring --test r3_voice_pack  # 10+10+10 passed
cargo test -p agent-vesper-tui --features voice-kokoro \
  --test r3_speech_worker --test voice_playback_diagnostics --test voice_speech_pipeline  # 6+2+1
cargo clippy -p agent-vesper-tui --all-targets --features voice-kokoro -- -D warnings    # exit 0
cargo clippy -p agent-vesper-tui --all-targets --features voice-conversation -- -D warnings  # exit 0
cargo clippy -p vesper-voice --all-targets --all-features -- -D warnings  # exit 0
cargo xtask architecture     # 28 packages validated
cargo xtask naming-guard     # clean (33 frozen hits)
cargo xtask acceptance       # 23/23 in 8356 ms
rustfmt --edition 2024 --check <the 8 files changed by this unit>  # exit 0
cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui
cargo run -p agent-vesper-tui --features voice-kokoro --example preview_first_pcm_receipt --release
cargo run -p agent-vesper-tui --features voice-kokoro --example first_speech_receipt --release -- --measure
```

## Exact evidence

### No-device CPU probes (current candidate preserved; machine load varies)

Preview worker (release, real installed Kokoro, no speaker):

```text
worker_ready_s=0.383
attempt=1 enqueue_to_first_pcm_s=1.693
attempt=1 enqueue_to_settled_s=3.020 samples=70800
attempt=2 enqueue_to_first_pcm_s=1.684
attempt=2 enqueue_to_settled_s=2.972 samples=70800
```

Long-unit first-PCM (release, fixed 188-byte fixture, no playback):

```text
warm whole-unit synthesis open: 8.974s; 188 text bytes; no playback
t=1.674s first PCM observed by fixture player
...
t=10.089s Spoke { segment: 1, samples: 247200 }
```

These sit inside the previously recorded post-repair ranges (Preview
1.559/1.569 s; long-unit ~1.62–2.59 s). They are software-delivery
receipts, **not** acoustic-onset or end-to-end latency measurements.

### Artifact identity

```text
binary: target/debug/agent-vesper-tui
sha256: d86389a3eed974195f26a256e904d43b7e68eb30b72ff39dc84309ca6d180633
build:  cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui
commit: 8f258ba28f4ea2f749526fb32b5b180ecc7dcead (HEAD; workspace has extensive
        pre-existing modified/untracked work — only the files listed below are
        attributed to this unit)
capability check: strings finds "Speech recognition compute",
        "compatible acceleration only", "NPU required"
```

## Files changed by this unit

Production:

- `crates/vesper-voice/src/execution.rs` (new, 601 lines)
- `crates/vesper-voice/src/lib.rs` (module + re-exports)
- `crates/vesper-voice/src/config.rs` (scope fields + parser + tests)
- `apps/agent-vesper-tui/src/voice_accel.rs` (new)
- `apps/agent-vesper-tui/src/lib.rs` (module registration)
- `apps/agent-vesper-tui/src/main.rs` (F9 gate resolution)
- `apps/agent-vesper-tui/src/settings_host.rs` (compute menus + readiness rows)
- `apps/agent-vesper-tui/src/settings_voice_save.rs` (policy persistence)

Verification:

- `apps/agent-vesper-tui/tests/voice_execution_policy.rs` (new)
- `apps/agent-vesper-tui/Cargo.toml` (test target entry)

Contracts/docs:

- `crates/vesper-voice/AGENTS.md`, `apps/agent-vesper-tui/AGENTS.md`
- `docs/voice-oracle-extraction-prd.md` (R16 clarification)
- `docs/foundation/evidence-index.md`, `docs/foundation/AGENTS.md`
- `docs/migration-status.md`

## Deviations and unresolved items

- **No physical NPU evidence is claimed.** No NPU route is registered; the
  AMD/FLM/XRT stack evidence from the prior assessment is machine-detection
  only. NPU STT and NPU TTS remain **open, unimplemented gates**.
- **The strict-NPU menu option is hidden, not shown-disabled**, in today's
  build (no registered route ⇒ no decorative control). It appears when a
  real adapter lands.
- The `r3_loop_pty.py` loopback PTY suite was **not re-run** in this unit
  (it exercises the unchanged F9/Kokoro dispatch path; its last recorded
  pass predates only this policy layer, which is covered by the new suite
  and the pr4/r3 Rust suites). It remains a recommended pre-release check.
- `cargo fmt --all` remains non-green **only** on pre-existing unrelated
  `ui.rs` lines (recorded in the prior report); all eight files changed by
  this unit are rustfmt-clean.
- Workspace/MSRV/five-platform/CI/release/install gates were **not run**
  (out of this unit's scope; remain pending for PR-5).
- The bounded no-device CPU probes reuse the installed approved Kokoro
  assets; no downloads, no devices, no NPU workloads, no saved settings
  were touched. The installed application was not replaced.

## User-operated acceptance prepared (open)

From `voice-user-latency-acceptance.md`, on the current candidate build:

1. **Preview**: cold first run + immediate warm repeat; click-to-first-audible.
2. **F9**: three turns; recording responsiveness; Stop; stop-to-transcript;
   stop-to-first-audible.
3. **Long response**: prompt first word; complete natural answer; no
   between-sentence gaps.
4. **Interruption**: barge-in during speech, then a later spoken turn still
   works; canceled output never resumes.
5. **Error path**: a truthful visible failure/retry, never silent loss.
6. **New for this unit (optional)**: Settings → Voice → "Speech
   recognition/synthesis compute" — verify CPU and Automatic rows behave,
   save persists across restart, and Readiness shows the two "using CPU"
   lines.

## Readiness effect

- CPU voice is preserved and re-probed on the current candidate
  (Preview/first-PCM receipts within prior ranges).
- The clarified R16 policy is **enforced in production**: CPU policy is
  structurally acceleration-blind; Automatic uses only verified routes;
  strict NPU refuses honestly per stage; stages resolve independently;
  stale copied strict preferences are revalidated and explained, never
  dispatched or rewritten.
- No-NPU machines get ordinary working CPU voice with no NPU installation,
  no acceleration requirement, and no voice-failure framing — proven by the
  no-attempt and zero-call regressions.
- VRO-17 as a whole remains **OPEN**: quantitative latency acceptance,
  repeated-interruption matrix, NPU STT, NPU TTS, and PR-5 remain separate
  open gates. This unit advances CPU acceptance evidence and lands the
  selection policy; it does not close the phase or advertise acceleration.
