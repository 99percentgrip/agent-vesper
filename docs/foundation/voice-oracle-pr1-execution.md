# VRO-17 PR-1 Execution — STT Adapters, Failover, Partials, Native-Engine Evidence

Work unit: implement PR-1 only — STT adapters, policy-constrained
failover, partial-transcript plumbing, and a bounded native-STT
feasibility assessment. **Stopped at PR-1.** No TTS, no voice-session
orchestration, no host capture/playback integration, no release work, no
paid-provider selection, no local-TTS decision (PR-2, Alex's).

## 1. Baseline

- Start revision `94ed16de502e98110498010b399b24659b17a63f` (`94ed16d`,
  2026-09-18 01:00:52 +0800), with PR-0's uncommitted work in the tree
  (crate + three planning/report docs + five index/DOX edits). All
  preserved and extended; no unrelated work touched. Everything below is
  uncommitted work on the same base revision — reported as such, not as
  committed history.
- Toolchain: rustc/cargo 1.95.0 (2026-04-14 / 2026-03-21); repo pins
  MSRV 1.88 via `rust-toolchain.toml` + `[workspace.package]`.
- PR-0 baselines **re-verified on this revision before building**:
  `cargo test -p vesper-voice` → 61/61 (48+13) green in 0.02 s. The
  cancellation-hang regression was inspected first
  (`cancel.rs` waker registration + `cancelled_future_resolves_after_cancel`
  with a 2 s timeout); it passes here and every PR-1 wait is deadline-
  bounded. Totals differ from the historical PR-0 report where suites
  gained tests (this is growth, not a discrepancy to paper over):
  see §8 for the reconciliation.
- External pin re-verified read-only: `88998de8369e9d36f6d434b5e01feb93fcf1c33f`
  (clean tree; no fetch). The worker's HTTP contract was re-read from the
  pinned source before the adapter was written (raw PCM body, optional
  an `x-`-prefixed shared-token header, `{"text"}` JSON, <0.1 s → empty,
  exception → empty).

## 2. Implemented (files, placement)

Adapter placement (unchanged from the corrected PRD §2.1): **inside
`crates/vesper-voice` behind default-off features, zero production
dependencies added** (D22). The pure core still builds bare.

- `src/composition/mod.rs` — composition namespace.
- `src/composition/blocking.rs` — `ValueExecutor` (dyn-safe erased
  submission), `ThreadPoolExecutor`, `run_blocking` (entry cancellation
  check; waker-aware `WaitSlot` bridge thread so the future stays `Send`
  and cancellable; honest limit documented: stopping the awaiter does
  not stop the computation — adapters bound it with deadlines).
- `src/composition/failover.rs` — `FailoverStt` (+`FailoverAttempt`
  sanitized trace, `AttemptOutcome`, `egress_permitted`,
  `FailoverConfigError`). Frozen policy executable: only `Unavailable`
  advances; silence/auth/quota/inference/malformed are terminal with
  their real classification; cancellation ends the request; per-attempt
  (and per-retry) egress recheck; attempt bound (no retry
  multiplication); loopback is not on-device evidence — class comes from
  the configured service's trust contract.
- `src/composition/partials.rs` — `PartialGate`: buffered-repass
  partials (`PartialsKind::BufferedRepass` advertised, incremental
  *not* claimed); whole-capture byte budget (loud
  `ResourceExhausted`, never silent truncation); one-queued-evaluation
  coalescing with a drop-guard; finals-first (finalization marks the
  generation stale before the final call); generation re-check after the
  blocking evaluation rejects stale partials; **no turn-submission API
  exists on the type** (partials cannot trigger agent turns).
- `src/stt_sidecar.rs` (feature `stt-sidecar`) — **Rust adapter with
  existing sidecar inference**: embeds the shipped script verbatim via
  `include_str!` from the TUI source (single source of truth), spawns a
  warm shared child lazily (never at construction; pre-start
  cancellation never spawns), own process group, `Drop` reaps exactly
  that PID (stdin close + TERM/KILL + wait; never broad name/port
  kills), bounded line reader thread (per-line byte cap, EOF flush),
  per-request deadline → `Unavailable`, error line → `Inference`,
  malformed → `InvalidInput`, empty finalized result with the script's
  `vad_filter=True` → `VadConfirmedSilence`, private temp WAV removed on
  drop, interpreter path validated against shell metacharacters/spaces,
  missing interpreter/model → `Unavailable` naming the setup
  prerequisite. No package installs, no model downloads.
- `src/stt_http.rs` (feature `stt-http`) — configured self-hosted
  worker adapter: endpoint validation (bare host, path, no
  userinfo/query/fragment), strict hand-rolled HTTP/1.1 over
  `std::net::TcpStream` (no redirects — 3xx is `Unavailable` with
  "redirect refused"; TLS is out of contract for self-hosted `http`),
  body/response/transcript bounds, 401/403→`Auth`, 402/429→`Quota`,
  malformed/missing/non-string/oversized→`InvalidInput`, transport→
  `Unavailable` (failover-eligible), <0.1 s captures rejected before
  the network (worker contract), empty 200 →
  `TranscriptProvenance::LegacyEmptyResponse` (**D21**).
- Contract amendment D21: `SttTranscript.provenance`
  (`InferredText` / `VadConfirmedSilence` / `LegacyEmptyResponse`) in
  `src/ports.rs`; fakes and PR-0 tests updated; the amendment and its
  rationale recorded in PRD §7.
- Contract amendment D22: `[features] stt-sidecar/stt-http` (default
  off, `[]` — no deps); PR-0's no-features manifest test amended to pin
  the new truth (features exist, default-off, no `dep:`).
- `tests/pr1_adapters.rs` — 30 tests over the **real adapter code**
  (subprocess fixtures via an executable wrapper speaking the shipped
  protocol; a controlled local TCP server speaking the pinned worker
  contract).

## 3. Adapter capability matrix (supported vs unsupported)

| Capability | `stt-sidecar` | `stt-http` |
|---|---|---|
| Final transcription | ✅ | ✅ |
| VAD-filtered silence as distinct outcome | ✅ `VadConfirmedSilence` | ❌ legacy worker discards it (D21) → `LegacyEmptyResponse` |
| Partials | ❌ (`partials: None`; the shipped script is final-chunked) — `PartialGate` wraps a partial-capable engine | ❌ (worker has no partial surface) |
| Egress class | `OnDevice` | `SelfHostedRemote` |
| Cancellation | ✅ pre-start + in-flight discard (computation-continues limit bounded by deadline) | ✅ same |
| Cleanup | ✅ child reaped on Drop; error/cancel cycles bounded | ✅ connection-scoped (`Connection: close`) |
| Auth | n/a (local) | ✅ token header; 401/403 → `Auth` |

Explicitly unsupported and *not* advertised: incremental partials, NPU
placement, TLS, redirect-following, cloud egress.

## 4. Native-STT feasibility verdict (documentation-based; no engine added)

Candidates assessed (≤2, current primary sources, 2026-09-18):

1. **whisper.cpp** (ggml-org, MIT, active: pushed 2026-09-18, v1.9.4) —
   C/C++ inference engine; GGML Whisper models (MIT-licensed
   Systran GGML conversions exist alongside the cached faster-whisper
   ones). Integration route for Vesper: process-boundary sidecar or FFI
   via bindings. VAD: built-in (its own VAD option) — parity with our
   binding would need verification. Verdict contribution: engine
   suitable in license and activity terms; integration effort is real
   (either a new sidecar binary or an FFI crate).
2. **rwhisper** (floneum/kalosm, MIT/Apache-2.0, active: pushed
   2026-09-14, `rwhisper` 0.4.1) — **pure-Rust** Whisper inference over
   the fusor runtime (CPU; GPU via fusor features). No pinned
   `rust-version` found in its manifest (MSRV fit unverified); pulls
   `cpal`, `rodio`, `tokenizers`, `fancy-regex` etc. (transitive
   surface needs a deny.toml pass); model path is HF-hub GGUF download
   (must be setup-gated, never auto-downloaded).

**Verdict: suitable for a later native implementation with named
prerequisites — not established as usable today, and nothing was
implemented.** Prerequisites for a future PR: (a) pick process-boundary
(whisper.cpp sidecar, licensing trivial) vs in-process (rwhisper, no
unsafe but heavy transitives); (b) verify MSRV 1.88 compatibility and a
`cargo deny` run for the chosen route; (c) a setup-gated GGML/GGUF model
path with no auto-download; (d) VAD-parity evidence (our binding is
`vad_filter=True` semantics); (e) measured latency/resource receipts
versus the sidecar. FFI-based options would be described as FFI, never
as pure-Rust inference. **No production engine, no `deny.toml` change,
no dependency install, no model download occurred in this unit.**

## 5. NPU carry-forward (unchanged, no new claims)

PR-0's record stands exactly: AMD XDNA device + `amdxdna` driver +
firmware 1.1.2.64 observed; **no userland inference runtime, no model
path, no offload evidence**. For the record: whisper.cpp advertises
vendor NPU routes on some platforms; rwhisper's GPU path is fusor-based
(GPU ≠ this NPU). Neither constitutes NPU support on *this* machine
without the missing userland. No drivers, runtimes, or firmware were
touched; no acceleration measurements are claimed anywhere.

## 6. Real-model probes (run; receipts on disk)

Script: `docs/foundation/voice-oracle-pr1-probe.py` (evidence helper,
not production code). It drives the **real shipped script** exactly as
the Rust adapter does (`python -c SCRIPT`,
`GLM_ACP_WHISPER_MODEL` env) against the **real cached models** on this
machine (`tiny`, `base` from `~/.cache/huggingface/hub`), through the
real protocol. Fixtures are synthetic (no recordings of Alex; no
microphone was opened): digital silence 30 s, digital silence 90 s (the
shipped long-silence hallucination shape), tone-burst 30 s (1 s of
440 Hz inside silence — **not intelligible speech**, honestly labeled as
a non-speech acoustic input; the mission's speech-fixture gate is
therefore **not met by this probe** and remains open for a
real-speech-fixture run — see §9).

Results (`docs/foundation/voice-oracle-pr1-probe-results.json`, 6 runs):

| Model | VAD | Fixture | Wall (s) | Outcome |
|---|---|---|---|---|
| tiny | `vad_filter=True` | silence 30 s | 1.60 | empty |
| tiny | `vad_filter=True` | silence 90 s | 1.45 | empty |
| tiny | `vad_filter=True` | tone 30 s | 1.02 | empty |
| base | `vad_filter=True` | silence 30 s | 1.34 | empty |
| base | `vad_filter=True` | silence 90 s | 1.98 | empty |
| base | `vad_filter=True` | tone 30 s | 1.41 | empty |

Acceptance applied: known digital-silence fixtures **must** transcribe
empty — all six runs did; no "You"-style hallucination appeared on any
silence fixture (the shipped regression holds through the adapter path).
No accuracy guarantee is claimed (no speech fixture); timing metadata is
recorded without transcript leakage. The adapter code path itself is
additionally exercised end-to-end by `pr1_adapters` with fixture
processes.

## 7. Cancellation/resource evidence

- Hierarchy under adapter use: parent→child propagation mid-flight
  (`cancellation_hierarchy_under_adapter_use` — outcome is either a
  completed pre-cancellation result or `Cancelled`, never a late Ok
  after observed cancellation); repeated cancellation idempotent;
  pre-start cancellation never spawns a child; the PR-0 waker regression
  (`cancelled_future_resolves_after_cancel`, 2 s timeout) remains green.
- Cleanup over cycles: 8 sequential failing adapters created/errored/
  dropped complete bounded (`repeated_error_cycles_do_not_leak_resources`,
  <30 s assert); `sidecar_drop_reaps_exactly_its_child` proves teardown
  does not hang on a lingering child (process-group kill).
- Deterministic synchronization: recv-timeout deadlines and scoped
  threads in tests; no sleep-only assertions.

## 8. Verification (commands and results, final state)

| Gate | Command | Result |
|---|---|---|
| Crate tests (default) | `cargo test -p vesper-voice` | 59+13+0 = **72 passed, 0 failed** (0.02 s) |
| Crate tests (features) | `cargo test -p vesper-voice --features stt-sidecar,stt-http` | 59+13+**30** = **102 passed, 0 failed** (~5 s incl. subprocess fixtures) |
| Pure-core bare build | `cargo build -p vesper-voice --no-default-features` | clean |
| Workspace all-features | `cargo test --workspace --all-features` | **170 targets, 2569 passed, 0 failed** |
| Workspace default | `cargo test --workspace` | 168 targets, 2433 passed, 0 failed |
| Doc tests | `cargo test --workspace --doc` | 28 targets, 0 failed |
| Strict Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| Formatting | `cargo fmt --all --check` | clean |
| Architecture | `cargo xtask architecture` | 29 packages (unchanged count; voice crate rules from PR-0 hold — no new deps were added) |
| Naming guard | `cargo xtask naming-guard` | clean (36 frozen hits) |
| Acceptance | `cargo xtask acceptance` | **23/23** in 6.15 s |
| Dictation regression | `cargo test -p agent-vesper-tui voice` | 1+7 passed, 0 failed (sidecar/VAD suites untouched) |

Baseline reconciliation: PR-0 reported 61 crate tests / 2528 all-feature
workspace tests / 23 acceptance. This unit re-ran PR-0's suite green
(61/61) before starting, then grew it: 72 default crate tests, 102 with
features, 2569 all-feature workspace tests across 170 targets (voice
crate contributes 102; growth elsewhere is none — 2528+41 new voice
tests = 2569). No gate was weakened; the naming-guard baseline is
unchanged from PR-0; no pre-existing failures were observed in any gate
this session. MSRV/cross-target/CI workflows: **not run** (CI-only
surfaces; recorded as pending — a green local Linux run certifies
neither other targets nor release readiness). Nothing was published or
tagged.

## 9. Unmet gates and honest limits

1. **Speech-fixture recognition receipt (open).** The mission's
   intelligible-speech fixture class has no approved asset in this
   workspace; the tone-burst probe explicitly does not substitute. A
   real-speech fixture run (synthesized TTS speech is acceptable, no
   recordings of Alex) is the remaining real-model gate. **This does not
   block PR-2** under the current plan (PR-2 is TTS/hygiene; STT speech
   evidence is tracked for the PR-4 real-device gate), but PR-1's
   *acceptance* on that axis is incomplete even though its *code* is
   complete and gated.
2. Native Rust inference: feasibility verdict only (§4); the sidecar
   path remains the labeled compatibility inference. Native-production
   acceptance is untouched.
3. NPU: unchanged PR-0 record (§5).
4. Cloud STT vendor: deferred to Alex; not selected, not advertised.
5. MSRV/target/CI: pending (§8).

## 10. Readiness for PR-2

PR-2 (TTS adapters, hygiene engine, egress enforcement) can start: it
consumes the same ports/cancellation/composition seams, and its open
input is Alex's local-TTS option (§2.7 PRD) — which PR-2 entry requires
but PR-1 did not. PR-1 leaves no blocking debt for PR-2; the open
speech-fixture receipt (§9.1) is tracked toward PR-4, not PR-2.

## Verification

- All §8 gates run on the final source state; instrumentation used
  during debugging (a dump env var + an eprintln) was removed and the
  suite re-run green.
- No live provider calls, no cloud credentials, no paid services, no
  audio capture of the user, no model downloads, no package installs,
  no NPU state changes.
- Status: PR-1 code complete and gated; speech-fixture recognition
  receipt open (§9.1). STT adapter completion is not working
  bidirectional voice.
