# VRO-17 PR-0 Execution — Contract Hardening and Core Implementation

Work unit: resolve the review findings against current source, correct the
VRO-17 architecture and migration contracts in place, and implement PR-0
only (core types, ports, descriptors, configuration contracts, in-memory
fakes, fixtures, architecture guards, and their tests). **Stopped at the
PR-0 gate.** No adapter, capture loop, model load, host UI, or release
work was started. PR-1 readiness is stated at the end with dependencies.

## 1. Baseline

- Workspace revision at start: `94ed16de502e98110498010b399b24659b17a63f`
  (`94ed16d` 2026-09-18 01:00:52 +0800 "docs: record v0.23.3 release
  execution"). Dirty state at start: the three VRO-17 planning documents
  (untracked) plus five modified index/DOX files — all from the previous
  planning unit, all preserved and extended here. No unrelated work was
  touched.
- External clone located as supplied by Alex at the sibling project
  directory (name withheld per the voice-oracle alias rule; recorded in
  the prior unit's report). Pin re-verified this session:
  `88998de8369e9d36f6d434b5e01feb93fcf1c33f` (2026-06-13 15:05:32 -0500),
  clean working tree. No fetch performed; no newer revision mixed in.
- Alias and no-port rule preserved. Upstream attribution/license text was
  never removed; the PRD now states explicitly that the naming rule
  governs Vesper-produced artifacts, not deletion of attribution (D-note
  in §"Reference upstream"). No license-notice conflict arose.

## 2. Commands and results (all run this session)

| Gate | Command | Result |
|---|---|---|
| Crate tests | `cargo test -p vesper-voice` | **61 passed, 0 failed** (48 unit + 13 contract), 0.02 s |
| Workspace all-features | `cargo test --workspace --all-features` | **169 targets, 2528 passed, 0 failed** |
| Workspace default | `cargo test --workspace` | 168 targets, 2422 passed, 0 failed |
| Doc tests | `cargo test --workspace --doc` | 28 targets, 0 failed |
| Strict Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| Formatting | `cargo fmt --all --check` | clean |
| Architecture | `cargo xtask architecture` | **29 packages validated** (was 28; +vesper-voice) |
| Naming guard | `cargo xtask naming-guard` | clean — 36 frozen hits; 2 new alias tokens, 3 pre-existing legitimate hits baselined |
| Acceptance | `cargo xtask acceptance` | **23/23 exact cases passed** (85 965 ms) |
| No-features build | `cargo build -p vesper-voice --no-default-features` | clean (pure core has no features by design) |

Pre-existing failures: none observed in any gate this session (the
85-second acceptance run and both workspace suites completed green before
and after the change). No gate was weakened; the naming-guard baseline
regeneration is the guard's own designed maintainer action for **newly
added tokens**, and the three frozen hits are pre-existing legitimate
mentions (a compatibility-instruction filename pair in
`project_context.rs`; a design-attribution comment in `scope.rs`) that
must be preserved — removal would damage compatibility and attribution,
and the baseline mechanism exists precisely for this case.

## 3. What PR-0 implements (files)

New crate `crates/vesper-voice` (pure core; `#![forbid(unsafe_code)]`;
deps exactly `vesper-domain`, `vesper-security`, `serde`, `serde_json`,
`thiserror`, `futures-core`):

- `src/audio.rs` — canonical format validation (`AudioFormat`), validated
  sample-aligned `PcmFrame` (odd length is a construction error), and
  `PcmReassembler` with odd-byte carry across arbitrary transport chunk
  boundaries and end-of-capture `Truncated` reporting. 6 tests.
- `src/cancel.rs` — `VoiceCancel`: waker-aware, idempotent, hierarchical
  children (parent cancels children recursively; a child cancel never
  touches parent or siblings). 8 tests, including multi-waiter wake and
  bounded-latency assertions.
- `src/error.rs` — the taxonomy: `Unavailable`/`NoSpeech`/`Auth`/`Quota`/
  `Cancelled`/`InvalidInput`/`Truncated`/`ResourceExhausted`/`Inference`,
  with `failover_eligible()` encoding "silence is an answer"; and
  `TtsMidStreamError::{AudioFailed,Cancelled}` for after-audio-began
  failures. Owned `String` labels keep it serializable. 2 tests.
- `src/ports.rs` — `VoiceStt`/`SttPartial`/`VoiceTts`/`TtsChunk`
  (+`Finished`)/`TtsStream`; `SttDescriptor`/`TtsDescriptor`/
  `VoiceProfile`; `PartialsKind::{BufferedRepass,Incremental}`;
  `SpeechEgressClass`; `FailoverPolicy` semantics (frozen contract;
  composition wrapper is PR-1).
- `src/events.rs` — complete host↔core vocabulary: `HostEvent` (capture,
  approved `AgentText`, `AgentTurnIdentity`, `AgentTurnSettled`,
  `PlaybackAck`, `PlaybackUnavailable`, `DeviceOrProviderFailure`,
  `BargeIn`, `StopRequested`) and `VoiceClientEvent` (partials,
  transcript, activity, `Speak`, `SpeechAudio`, `PlaybackStop`,
  `Interrupted`, exactly-once `TurnDone`, `Failed`);
  `PlayState::{Draining,Acked,Stopped,Unknown}` (D5 honesty);
  `SpeechSegmentId` correlation; `project_phase` dimension projection. 6
  tests.
- `src/report.rs` — `VoiceTurnReport` (metadata-only by construction; no
  field can carry transcript/audio) + `ReportMarker` truthfulness
  markers. 2 tests.
- `src/config.rs` — `[voice]` TOML scope (enabled/stt/tts/partials/voice/
  egress), `SpeechEgress` policy with `validate_egress` (on-device never
  reaches remote classes; self-hosted never reaches cloud), and
  `CaptureBudget` bounding capture/pending-text/queued-audio. No hygiene
  disable key exists in the schema (D9 structural). 14 tests.
- `src/fakes.rs` — `FakeStt` (scripted outcomes incl. NoSpeech/
  Unavailable/Inference; best-effort partials; fixture-marked
  observations) and `FakeTts` (clean finish / mid-stream failure /
  open-failure / hang-until-cancelled scripts) implementing the full
  port lifecycles. 9 tests.
- `tests/pr0_contracts.rs` — 13 cross-module contract tests: manifest
  purity (no adapter deps in `[dependencies]`), canonical PCM fixedness,
  truncation visibility, error classification, egress policy rejection,
  no-hygiene-switch, partial-kind labeling + final supersession, TTS
  lifecycle tri-distinction, waker-aware bounded cancellation,
  phase-projection overlap, report metadata-only, settlement enum
  ownership. Includes a runtime-agnostic `block_on` so contract tests
  pin no executor.
- `Cargo.toml` — registered in workspace members; arch allowlist entry
  added to `xtask/src/main.rs` (29 packages); naming-guard alias tokens
  added (11 → 13 tokens, hex-encoded as the mechanism requires).

Explicitly **not** implemented: adapters (local/cloud), capture loops,
model loading, runtime activation, host UI, live audio of any kind. The
fakes' passing is contract evidence only — it is not evidence that
production cancellation, synthesis, or recognition works, and nothing in
this unit claims a provider exists.

## 4. Local TTS dependency verification (D13, primary sources)

The previous "Piper (MIT)" table entry was **refuted** by checking the
current primary sources (GitHub API + raw license files, this session):

- Original upstream repo: MIT, **archived** (archived flag true; last
  push 2025-08-26). An archived engine is not the maintained line.
- Actively maintained successor (`OHF-Voice/piper1-gpl`, last push
  2026-09-17): `COPYING` = **GPL-3.0**; README states it embeds
  **espeak-ng** for phonemization (GPL family); a bundled
  `licenses/LICENSE.g2pW-Apache-2.0` shows mixed per-component terms.
- Workspace policy (`deny.toml`) permits only permissive licenses
  (Apache-2.0/BSD/MIT/ISC/BSL-1.0/…); GPL-3.0 is not among them.
  Distribution of GPL-3.0 code would change the supply-chain posture;
  FFI or process isolation does **not** change license obligations.

Decision recorded in the PRD (§2.7): local TTS is a **gated candidate**
with three explicit options for Alex at PR-2 entry — (a) a
permissive-licensed engine after equivalent verification, (b) the GPL
engine used only as an opt-in **user-installed local executable** behind
a process boundary, with license disclosure and zero distribution by
Vesper, or (c) deferral with a clearly labeled cloud-only preview that
explicitly cannot satisfy R3 without a recorded scope change. No paid
service was selected; no engine is named as chosen. Distribution/
integration implications: option (b) would also need a dependency-setup
path that installs user-local software without Vesper redistributing it.

## 5. NPU readiness record (D16, read-only)

The mission said the laptop NPU is enabled. Inspected via sysfs only —
**no driver change, no reinstall, no firmware, no credentials, no model
downloads**:

- Device present: `/sys/class/accel/accel0`, PCI `1022:17F0`
  (AMD), class `118000` (processing accelerator).
- Kernel driver bound: `amdxdna` (`/sys/class/accel/accel0/device/driver
  → ../../bus/pci/drivers/amdxdna`), kernel `7.2.5-200.fc44.x86_64`,
  driver license GPL (in-tree kernel module).
- Firmware: `fw_version` = `1.1.2.64`.
- CPU: AMD Ryzen AI 9 465 w/ Radeon 880M (XDNA-class NPU silicon).
- Userland/runtime: **none present** — no `xpu-smi`/NPU tools on PATH,
  no `onnxruntime` Python module, no ONNX/voice NPU runtimes under
  `~/.local/share`; no existing workspace NPU integration (grep across
  crates/apps: no accelerator layer exists to build on).
- Kernel log: no xdna probe errors; boot did not surface NPU userland.

Conclusion recorded in PRD R16: **device detected + driver bound +
firmware version read; no backend, no model, no inference evidence.**
Therefore: no NPU-backed capability claim anywhere. Distinguished
ladder (for any future initiative): device detected → backend available
→ model loadable → inference actually offloaded (per stage: VAD / STT /
TTS separately, since offload differs). A future bounded probe plan
(install-free): query the accelerator's queryable interface via the
kernel's uAPI when a userland library exists; compare CPU vs NPU latency
only when real measurements exist. Existing safe probe: none found, so
the missing evidence is recorded rather than fabricated. This
target-specific record is independent of generic PR-0 readiness.

## 6. PRD corrections (summary; details in PRD §7 decision record)

Twenty findings resolved (D1–D20): each recorded as confirmed/already
handled/refuted with source evidence, the required change, and its
acceptance test. Highlights: dependency contradiction (D1) resolved by
voice-owned host-translation events; cancellation ownership (D2) by the
locally owned waker-aware token; adapter placement (D3) by the pure-core
boundary with per-PR evaluation; overlapping dimensions (D4) and honest
playback acks (D5); TTS mid-stream lifecycle (D6); partial-kind labeling
(D7); NoSpeech-vs-Unavailable split (D8); non-disableable cloud redaction
(D9); hygiene/gating ordering as one stateful bounded pipeline (D11);
R2 wording (D12); Piper license refutation (D13); R3 contingency
labeling (D14); capture-scope wording (D15/R13); NPU (D16/R16); whole-
capture budgets (D17); upstream ack feature stays refused (D18); R9
re-scoped to deterministic observable surfaces (D19); preserved stronger
behaviors restated (D20). R1–R12 identifiers preserved; R13–R16 added
traceable. PRD §8 adds the reference→requirement→owner→test→deviation→
stage traceability matrix. Historical recon/execution documents were not
rewritten.

## 7. Deviations and corrections from this unit's own process

- A contract test initially hung (60+ s) because the first `VoiceCancel`
  draft returned `Poll::Pending` without registering a waker and the
  fake's wait path busy-looped. Alex flagged the stuck run. Fixed by
  making the token waker-aware (wake-once, recursive child propagation)
  and bounding every wait in tests with explicit deadlines; the suite now
  completes in 0.02 s. The failed intermediate design is recorded here,
  not hidden.
- A first draft of `child()` shared state with the parent (child cancel
  would have cancelled the turn). Caught by this crate's own unit test
  (`child_cancellation_does_not_touch_parent`), fixed with weak-child
  registration + recursive propagation + sibling isolation tests.
- One clippy-driven refactor (`.is_multiple_of`, slice-from-ref) and one
  scanner-driven doc rewording (removed a `vesper-testkit` mention from
  a comment) — no behavior change.

## 8. Deferred to PR-1–PR-5 (allocated, not implemented)

Bounded queues enforcement; partial/final ordering with final priority;
overlapping generation/playback state machine; stale-generation
rejection; playback-ack correlation; cancellation-before-runtime-identity;
repeated cancellation idempotence; terminal-outcome uniqueness
enforcement; policy-constrained failover execution; cross-chunk hygiene
engine + canary fixtures; adapter cleanup proofs; host baseline snapshot
(R9); real-device acceptance incl. assistant-audio-retranscription case;
ACP capability re-check. Later-phase receipts planned but not yet
measured: end-of-speech → first audible output; first generated vs first
played audio; interruption → playback stop; cancellation settlement;
sustained resource use. Planned/skipped tests are not counted as passing
evidence anywhere in this report.

## 9. Open decisions for Alex (non-blocking for PR-1 start)

1. Local TTS option (a)/(b)/(c) — decide at PR-2 entry (PR-1 has no
   dependency on this).
2. Cloud STT/TTS vendors — whenever convenient before PR-1/PR-2 gates;
   nothing is named or advertised until then.

## 10. Readiness effect

PR-1 can start immediately against the frozen contracts in this crate
(`VoiceStt`/`SttPartial`/`VoiceError`/descriptors/fakes; failover
semantics already pinned). PR-1 dependencies: the existing Python
sidecar protocol (already shipped and tested) for `whisper-local`, an
HTTP shape for `stt-remote-http`, and a native-engine spike verdict.
Nothing in PR-0 claims voice capability for either host: with no voice
configuration, both hosts remain exactly as they were (no registry,
startup, or protocol change has landed — the crate is not yet referenced
by any host, which is itself the R9 trivial baseline at this stage).

## Verification

- All gates in §2 run after the final source state; results quoted from
  the final runs.
- No live provider calls, no user-state writes, no audio captured, no
  models downloaded, no NPU state altered.
- Status: PR-0 complete; PR-1–PR-5 not started. Contract completion is
  not working voice.
