# Migration Foundation Evidence

## Purpose

Own evidence and decisions that close the pre-workspace blockers identified by reconnaissance.

## Ownership

- `2026-09-24-critical-tool-output-stall-repair.md` owns the permanent shared
  command-output settlement repair evidence, provider/tool-surface
  reconnaissance, host-route tests, platform limits, and remaining live
  provider-correlation gate.
- `2026-09-25-tool-output-stall-platform-verification.md` owns the portable
  Windows/macOS settlement-matrix preparation, Job Object/process-group repair,
  CI-ref constraint, local red-to-green follow-up and exact remaining gates.

- `mcp-session-lifecycle-repair.md` owns persistent MCP source repair,
  isolated Playwright continuity evidence, host wiring and platform limitations.

- `v0.22.6-release-execution.md` owns the microphone release gate and publication receipts.

- `voice-control-execution.md` owns dynamic microphone implementation, long-audio
  lifecycle evidence and explicit real-device/platform acceptance limits.

- `voice-control-recon.md` owns the missing-footer regression diagnosis and
  requested dynamic microphone control implementation boundaries.

- `v0.22.5-release-execution.md` owns guided setup publication and exact-commit release evidence.

- `dependency-setup-execution.md` and `dependency-setup-verification.json` own guided
  dependency setup implementation,
  isolated readiness evidence and explicit missing clean-platform acceptance.

- `dependency-setup-recon.md` owns the dependency-bootstrap investigation and
  proposed native setup flow. The requested UX is automatic preparation for
  selected features, without normal users copying shell commands or editing
  configuration; OS consent and actual readiness checks remain explicit.

- `v0.22.4-release-execution.md` owns the routing-preview release, exact-commit
  gate and registry receipts, documentation refresh and preserved local installation.

- `settings-and-update-execution.md` owns native Settings, automatic enrollment,
  updater repair evidence and unexecuted platform acceptance.
- `v0.22.2-release-execution.md` owns the Settings repair release receipts and
  initial installation evidence and pending user update test.
- `v0.22.3-release-execution.md` owns visual-upgrade release receipts and proof
  that the installed 0.22.2 payload was preserved for user updater testing.
- `output-visual-upgrade-execution.md` and `output-reference-*.png` own native
  output upgrade verification and actual renderer reference captures.
  `output-reference-render.py` rasterizes captured cells with Linux Noto fonts; it
  is an optional Pillow-based evidence helper, never production rendering.
- `skill-routing-quality-implementation.md` owns the isolated preview routing
  implementation, quality HOLD/ADOPT evidence and score-floor integration dependency.
  `skill-routing-quality-results.json` retains frozen offline predictions;
  `skill-routing-quality-summary.py` derives metrics without changing labels.
- `skill-routing-language-execution.md` owns the follow-up language normalization,
  development measurements, unchanged-corpus evaluation and remaining gates.
  `skill-routing-language-results.json` preserves its complete prediction receipt.
- `skill-routing-request-recognition-execution.md` owns request-recognition
  experiments, sourced language-asset reproduction and retained failed receipts.
- `skill-routing-embedding-experiment.md` and `skill-routing-embedding-*` own
  optional offline embedding probes and their unpromoted result receipts.
  They never form a native runtime, library rewrite or automatic model download.
- `skill-routing-independent-*` owns the evaluator-authored frozen corpus,
  authoring provenance, native predictions and separately scoped static reviews.
- `skill-routing-model-assistance-execution.md` owns native selector implementation,
  offline boundary/host verification and separately authorized live quality evidence.
  `skill-routing-model-verification.json` binds source files and offline receipt hashes.
  `skill-routing-model-live-evaluation.md` and its summary helper separate live
  decisions, native fallback, unsupported contexts and measured usage; inspected
  regression cases never become unseen holdout evidence.
  `skill-routing-current-quality-results.json` retains current four-size lexical
  ablations, including failed gates.
- `evidence-index.md` is the durable execution ledger and command record.
- `completion-assurance-proposal.md` owns the researched proposal for native
  requirement coverage, execution receipts, and enforced completion decisions.
  It records inspected sources and the approved acceptance criteria.
- `completion-assurance-execution.md` owns ADR 0028 implementation evidence,
  executable coverage, measured limits and release readiness.
- `vro14-gap-audit.md` owns current web-extraction gaps, repair evidence,
  deployment prerequisites, and outstanding release acceptance.
- `vro15-gap-audit.md` owns independent swarm-extraction acceptance findings,
  verification limits, and the original repair proposal.
  `vro15-repair-execution.md` tracks Alex's approved full repair scope, gates and
  execution evidence, including VesperLens provider-neutral feedback delivery.
  `vro15-codex-repair-prompt.md` is the external coding-agent handoff for remaining
  repairs; the PRD, accepted ADRs and current execution matrix remain authoritative.
  `vro15-audit-probes.rs` is standalone non-production evidence: its assertions
  reproduce defects, not desired behavior; compile/run as documented in the report.
- ADRs under `adr/` record Stage 0 compatibility and product choices.
- `memory-oracle-cognitive-memory-blueprint.md` is the reconnaissance record for the
  external the memory oracle (`29fa4155`) oracle and the evidence base for ADR 0015
  (Stage 16 — `vesper-cognition`). The the memory oracle oracle is independent of the
  frozen Python harness; this is the only place where the memory oracle is cited.
- `vro13-pr8-closeout-evidence.md` is the VRO-13 cross-feature closeout record:
  the end-to-end fixture (`crates/vesper-harness/tests/vro13_e2e.rs`), the
  pipeline coverage (watcher/cron fire → composed firewall → sandbox route →
  scope-keyed transcript), and the PR-1..PR-8 verification trail per
  `docs/qm-extraction-prd.md`.
- `vesperlens-end-to-end-acceptance-postmortem.md` is the owner-directed
  VesperLens audit (v0.20.29 → v0.20.44): the binding end-to-end completion
  standard, the ten audited gaps, honest-scope list, and the open task of
  wiring the real-browser scripts into verification.
- The remaining reports document source-baseline diagnosis, fixture/oracle results, disposable Rust spikes, and readiness.

- `voice-user-latency-acceptance.md` owns Alex's native recording, first-spoken
  and Preview timing observations, the failed repeated-turn playback result on
  the CPU acceptance candidate (later repaired; speech restored), the
  FAILED/OPEN continuity and phonemization statuses at that record's date
  (both since repaired and superseded by later acceptance records), and the
  retest matrix the voice phase required at that time.

- `voice-kokoro-cold-warm-starvation-investigation.md` owns the cold/warm
  starvation measurement (2026-09-23/24): the fixed-text matrix receipts,
  the paced-player EOF-vs-audio-duration starvation proof (zero events),
  the traced worker/engine/bank/player lifetimes (per-process host on
  first F9 gesture; per-turn reuse; per-piece espeak+ORT), the cold
  onset numbers, the NOT_REPRODUCED classification with the honest
  secondary mechanism (cold first-piece onset), the no-repair decision
  and the bounded follow-up options. The measurement harness lives at
  `apps/agent-vesper-tui/examples/cold_warm_starvation.rs` (test-only
  instrument, never production).

- `voice-short-reply-quality-repair.md` owns the remaining VRO-17 voice-quality
  repair: real-path proof that short-reply stops were stacked Kokoro waveform
  padding rather than player starvation, short-unit-only guarded edge cleanup,
  the rejected broad trim and restored long-form continuity evidence, the
  clarified Live transcript preview wording, regression/PTY receipts, and the
  immutable combined-feature candidate, and Alex's later bounded “Looks like
  smooth” acceptance on the tested setup.

- `voice-kokoro-pronunciation-investigation.md` owns the 2026-09-23
  pronunciation investigation: the deterministic reproduction of Alex's
  three word defects, the boundary-by-boundary exonerations
  (hygiene/segmentation/IPA-postprocess/token-mapping/resampling), the
  proven espeak-ng stdin-truncation root cause, the one-line
  terminating-newline repair with red→green receipts (and the recorded
  vacuous-red correction), the bounded listening A/B artifact location,
  and candidate `agent-vesper-tui-pronunciation-repair` (3b1bb9d3…).
  The repair is subsequently user-accepted on the tested path with no new
  pronunciation regression reported in the short-reply retest. Residual espeak
  G2P limitation recorded; any pronunciation-override feature requires separate
  approval.

- `voice-r6-device-interruption-acceptance.md` also owns the 2026-09-24
  §2.4 pre-acceptance blocker: the source-traced gesture→effect mapping
  (Speaking+F9 = speech-mute only; genuine BargeIn session-side on the
  second press; Ctrl+C runtime-cancel without speech stop; the correct
  `ConversationHost::stop()` unreachable from production), the
  implementation-open reclassification, the withdrawn device brief, and
  the smallest existing-owner correction spec. This is explicitly historical:
  the binding repair and later device PASS supersede it. The audit's earlier
  "F9 wiring verified" reading mistook session-layer readiness for host
  execution — corrected in place.

- `voice-r6-binding-repair.md` owns the implementation that closes that §2.4
  production blocker: red-first production-entry proof, direct genuine-BargeIn
  F9 routing, explicit Stop routing, runtime/playback/synthesis/capture effects,
  provider-neutral and voice regressions, repository gates, and immutable
  candidate identity. Its verdict at that unit's date was implementation-complete
  with real-device interruption acceptance pending; the later device closeout
  passed. It does not itself run the device matrix or PR-5.
  The earlier blocker section remains the historical pre-repair finding and
  must not be rewritten as if it had observed the repaired source.

- `voice-r6-device-interruption-acceptance.md` owns the R6
  Alex-operated device acceptance: the pre-session artifact identity
  (candidate + SHA + provenance), the read-only production-path
  verification receipts, the launch/Settings instructions, Tests A–D,
  and Alex's observations + verdict. The final 2026-09-24 result is PASS on the
  tested setup: one-press/repeated interruption, explicit Stop and later-F9
  recovery passed; old speech did not resume and Stop opened no capture. The
  pre-repair withdrawn brief remains labeled historical.

- `voice-r6-device-acceptance-closeout.md` owns the documentation-only R6
  closure work unit: branch/revision/dirty-state record, final device matrix,
  §2.4 reconciliation across automated and device evidence, Alex's Ctrl+C/old-
  speech clarification, bounded voice-quality/pronunciation acceptance,
  corrected 22-row requirement accounting, document checks and the PR-5-only
  remaining set.

- `voice-r4-optional-partials-execution.md` owns the R4 closure (2026-09-23,
  Alex-approved Option B): the amended optional-capability contract, the
  re-verified final-only capability table (sidecar/HTTP/FLM), the
  capability-aware Settings behavior (`voice_accel::partials_settings_row`
  + guarded toggle; preference preserved), the red-first proof pattern
  (production row fn reverted → suite fails), and the two-fake-provider
  reasoning-neutrality regression that closed the decision record's test
  gap. Candidate `agent-vesper-tui-r4-optional-partials` (673f9fb8…).

- `voice-r4-partials-scope-decision.md` owns the R4 A/B decision record
  and the reasoning-provider-neutrality audit (2026-09-23): the verified
  provider-neutral call path, the provider-name sweep classification
  (GLM_* speech-env names = legacy naming, not reasoning coupling), the
  specified-but-unimplemented two-fake-provider parity regression, the
  source-verified R4 reality (all adapters final-only; PartialGate
  host-unreachable; dormant `scope.partials`; no interim UI), and the
  Option-B recommendation that awaited Alex at that unit's date. The later R4
  amendment resolved it. Decision-only unit: it changed no production code and
  did not itself amend R4.

- `voice-r20-default-capture-repair.md` owns the R20 closure (2026-09-23):
  the source-verified finding that BOTH build flavors wrote captures to an
  uncapped tempdir (the feature build's `self.managed` was dead state), the
  one-store/one-ownership repair (recorder streams through the capped
  writer on Linux; afrecord writes the store path on macOS; cap-vs-failure
  recorder-exit classification), the red→green default-build suite that
  runs in every feature set, the streaming-seam fixture modernizations
  (`voice_pty`/`r3_loop`/`flm_f9_loop`/`voice_recorder_fixture`/
  `voice_model_fixture`), and candidate
  `agent-vesper-tui-r20-default-capture` (70cfb122…). F5 semantics and the
  F9 path are unchanged; audible behavior untouched.

- `voice-vro17-final-completion-audit.md` owns the 2026-09-23 evidence-led
  completion audit of the whole PRD: the R1–R20 verdict matrix with
  source-verified reachability facts (no cloud STT/TTS adapter exists;
  `PartialGate`/`FailoverStt` are library-only; ACP has no voice surface;
  the default-build dictation path is uncapped in source), the phase and
  external-gate tables (PR-5 open; all post-v0.23.3 voice work is
  uncommitted, so no release evidence covers it), the preserved user
  timeline, and the queued documentation-drift list. Its §G corrections
  are the authorized scope of a later documentation unit — the audit
  itself changed no status text. **Amended 2026-09-23 (Alex-approved):**
  R2 cloud STT and R3 cloud TTS are optional gated integrations; R16b
  NPU TTS is capability-gated with CPU synthesis first-class/conformant.
  The audit's §B/§H reflect the amended contracts with explicit counting
  (22 matrix rows / 20 requirement IDs; phases separate). After the R4, R20
  and R6 closeouts the current ledger is PASS 17 · PASS-scoped 4 · optional-
  conformant 1. All R1–R20 requirements are closed at approved scope; PR-5 is
  the only remaining phase gate.

- `voice-pr5-host-release-execution.md` owns the final VRO-17 PR-5 unit:
  ACP's protocol-backed audio/control exclusion, cross-host provider/runtime
  parity, the release-feature omission and repair, v0.23.4 exact-commit gates,
  publication assets/checksums and registry status. Cloud STT/TTS remain future
  optional adapters behind `VoiceStt`/`VoiceTts` and their provider-specific
  security and acceptance gates.

- `voice-npu-user-acceptance.md` owns the 2026-09-23 real-device user
  acceptance of the tested NPU-enabled conversation experience: Alex's
  verbatim verdict, its perceived-continuity scope (not a measured
  zero-gap, placement, or NPU-TTS claim), the explicit tested-artifact
  identity gap (both live TUIs run an FLM-free installed binary; the
  10:03/10:09 listening sessions have no launch receipt), the preserved
  candidate table, and the still-open gates. It supersedes the earlier
  "good enough for now" verdict only as a present status; that result and
  the ~5.2 s cold-start measurement stay as history.

- `voice-npu-user-acceptance-closeout.md` owns the documentation-only work-unit
  report for that acceptance: authorized scope, exact verbal receipt,
  configuration regression baseline, executable-identity exclusions, session
  drift, verification, unresolved gates, and readiness effect. It must not turn
  a later-built candidate or earlier live-process observation into the tested
  artifact.

- `voice-latency-performance-repair.md` owns the post-acceptance candidate repair:
  smaller first/successor synthesis pieces, the reusable Settings Preview worker,
  red→green regression, device-free timings, rejected four-thread experiment and
  the still-open native/acoustic acceptance boundary. The capability-gated
  selection policy (`voice-capability-gated-execution.md`) preserves this
  candidate unchanged.

- `voice-npu-integration-execution.md` owns the R16 reconnaissance gate:
  FLM 1.0.5's concrete standalone Whisper endpoint, the verified missing
  VAD/silence and cancellation contracts, the absent consent-gated model, and
  the independently unestablished Kokoro Linux-NPU route.
- `voice-npu-stt-implementation-progress.md` owns the subsequent implementation
  gate: the persistent installed-Silero VAD worker and regression (both silence
  and speech-positive paths), the explicitly authorized/hash-verified FLM
  Whisper asset, the initially failed launch receipts, and the 2026-09-22
  launch-gate correction proving FLM 1.0.5 standalone ASR (`flm serve --asr 1`,
  no positional chat model) serves real NPU transcription on loopback with no
  downloads. Backend evidence is not integration: adapter registration still
  requires the VAD-protected composition adapter, owned lifecycle, cancellation
  suppression and production-path tests; CPU routes remain authoritative.

- `voice-first-speech-and-npu-assessment.md` owns the authorized live-turn timing,
  local long-unit early-PCM repair, visible running-stage regression, current AMD
  XDNA/FLM/XRT evidence and separately OPEN native NPU STT/TTS implementation gates.
  Its fixture/code verification does not close the later failed user latency
  acceptance. It supersedes the historical no-userland-runtime note, not the
  offload gate.

- `voice-capability-gated-execution.md` owns the clarified-R16 capability-gated
  selection policy: pure-core stage execution policy (CPU / Automatic-verified-only
  / strict-NPU), evidence-based readiness facts, in-process readiness caching,
  honest offload placement attribution, the honest empty route registry, F9 gate
  resolution, Settings compute menus, and the no-NPU regression matrix. It is
  selection policy, not NPU implementation; both NPU stage gates stay open.

- `voice-cpu-production-acceptance.md` owns the running-CPU-application
  verification: the PTY loop regression in all three modes with pre-verified
  isolation, the Preview/F9 policy-parity repair (`voice_accel::preview_policy_gate`),
  the no-op-reload engine-retention repair, the pack-screen action-index repair
  the loop check caught, the interruption/error/lifecycle production-path suites
  (`voice_policy_parity`, `voice_interruption_lifecycle`), release-profile
  stage-distinct timings, the preserved identity-verified candidate artifact,
  and the NOT-RUN Alex-operated acceptance checklist.

- `voice-multiturn-playback-repair.md` owns Alex's failed repeated-turn
  playback result and its repair: the `--fatal-errors` argv correction (aplay
  aborts on recoverable xruns by that flag's documented behavior — the
  established mid-stream child-death mechanism under per-piece feeding),
  original-write-error preservation, dead-stream containment with next-turn
  recovery, short-consumer honesty, the `voice_multiturn_playback` suite
  (realistic-rate controlled players; ten consecutive single-process turns;
  turn-3 death + recovery), the separately-open STT garbling observation, and
  the device retest that confirmed speech restoration.

- `voice-continuity-phoneme-repair.md` owns the implemented continuity and
  formatting-unit corrections: the superseding subdivision rule (clause-sized
  onset piece + sentence-level successors within the 510-ID budget; the
  28/48 decision is historical), the context-aware list-marker/table-rule/
  heading-delimiter handling in the hygiene gate (`leading_list_marker`,
  marker-aware sentence end, prefix deferral, `formatting-only` marker), the
  mandatory meaning-preservation distinctions, the paced production
  measurements (stacked quiet 7.2 s → 2.9 s; one quantified cold-start gap
  with a separately-proposed bounded prebuffer NOT applied), the open
  decimal-split finding, and the pending listening comparison on the
  `agent-vesper-tui-continuity-phoneme-repair` candidate (9d59189a…).

- `voice-continuity-phoneme-recon.md` owns the measured continuity/phoneme
  reconnaissance after speech was restored: the pause timeline (stacked
  fade-in/out quiet runs at 28/48 mid-sentence piece boundaries — 7.2 s of
  inserted silence in a 41 s passage; whole-sentence comparison at 2.9 s on
  natural pauses), warm-vs-cold RTF margins with the paced-sink continuity
  receipts, the empty-phoneme reproducer (`"1."` numbered-list hygiene units
  map to zero vocab ids; 22 zero-id symbol characters; piece-boundary
  stranding ruled out), the corrected EPIPE wording, the isolated remedy
  comparison (both remedies now implemented by the repair record above), and
  the red-first acceptance that repair satisfied. Diagnostic examples are
  isolated recon helpers, never production code.

- `voice-speech-pipeline-repair.md` owns bounded synthesis/playback overlap,
  current primary-source comparison, red→green ordering/Stop evidence and the
  user-confirmed removal of the two-sentence playback gap. Initial live coding
  reply latency remains open; fixture and player timings cannot close it.

- `voice-verify-read-failure-repair.md` owns the 2026-09-23 Verify-reliability
  repair: the verbatim reproduction of Alex's user-tested `owned ASR read
  failed or timed out` through the native Settings route, the vendor-log
  mechanism proof (orphaned owned-shape FLM servers exhaust NPU device
  contexts; `DRM_IOCTL_AMDXNA_CREATE_HWCTX (EINVAL)`; the collapsed read
  label misreported the immediate reset as a timeout), the red→green
  error-kind classification in `voice_flm.rs` (`process exited while
  answering` vs `read timed out`; no deadline change), the pure-std durable
  child registry + registrar-host-liveness reaper (dead-host orphan reaped,
  live-host child proven untouched, no unsafe), the transport test-seam
  locking rule, and the candidate `agent-vesper-tui-verify-read-repair`
  (b289a6c1…). `settings_pty.py`'s submenu-click drift on every preserved
  candidate is recorded there as pre-existing.

- `voice-continuity-boundary-repair.md` owns the 2026-09-23 recording-review
  unit: the decoded recording stage timeline, the rendezvous/drain
  serialization mechanism with its red 1.019 s boundary-gap reproduction and
  the derived depth-1 insufficiency, the bounded depth-2 bank in
  `voice_speech_worker.rs` (supersedes this list's depth-0 "one-unit
  lookahead" description — the overlap contract and all Stop/FIFO bounds
  are unchanged), the restored `VESPER_PYTHON_PATH`/`GLM_VENV_PATH`
  precedence in the conversation-CPU adapter (a silent CPU-route regression
  from the FLM integration that only the legacy PTY loop caught), the
  scope-aware readiness wording plus the real last-request route receipt,
  the chdir-race and verification-state test-isolation repairs, and the
  candidate `agent-vesper-tui-continuity-boundary-repair` (fa0c3713…).
  The acoustic-listening boundary this unit left open is superseded as a
  present status by `voice-npu-user-acceptance.md` (2026-09-23) and kept
  here as history.

- `voice-latency-repair.md` owns repeated-hashing and recorder-import latency
  repairs, fresh-root capture correction, local timing/regression evidence and
  still-open live end-to-end latency acceptance.

- `voice-kokoro-pcm-scaling-repair.md` owns the primary-source integration audit,
  real-model near-zero signal reproduction, normalized-float conversion repair,
  direct/VRO signal checks and user-confirmed corrected playback.

- `voice-kokoro-authorized-playback.md` records the single user-authorized real
  output probe and Alex's negative original listening result; the subsequent PCM
  repair owns corrected playback evidence. Full native acceptance is open.

- `voice-playback-diagnostics-repair.md` owns the device-free shared-player
  diagnostic regression and repair; later PCM/playback reports own audibility
  evidence rather than inferring it from diagnostic success.

- `voice-oracle-local-neural-audition.md` owns the Kokoro-82M
  feasibility assessment: listening options (creator samples + demo
  Space), runtime-route comparison (reference Python rejected; ONNX +
  `ort` recommended), exact asset manifest (~87 MiB), and the
  integration proposal. No downloads or integration executed.
- `voice-oracle-kokoro-implementation.md` owns the R3 Natural Voice
  pack execution: the dedicated `vesper-voice-kokoro` adapter crate
  (pack descriptor/integrity/lifecycle, pronunciation bridge, bounded
  engine), the TUI `voice-kokoro` feature (Settings install/preview/
  repair/remove + engine selection + shared F9 assessment), pinned
  assets/runtime with the recorded q8f16→model_quantized decision,
  real setup + performance receipts, budgets held, red→green tests,
  and the still-open Alex listening/F9 acceptance. Section 13 supersedes the
  initial completion claim with the production-routing repair, isolated
  provider-wire/real-inference receipts, and outstanding acceptance limits.
- `voice-oracle-pr4-repair.md` owns the PR-4 production binding repair
  (before/after caller map, red→green wiring tests, Settings panel
  evidence, honest R20 coverage, artifact identity).
- `voice-oracle-pr4-execution.md` owns VRO-17 PR-4 (TUI integration,
  playback-evidence contract, R20 capture policy/tests, the synthetic
  fixture STT receipt via `voice-oracle-pr4-stt-gate.py` + results, and
  the user-operated acceptance checklist that remains open).
- `voice-oracle-pr3-execution.md` owns VRO-17 PR-3 (voice-session
  orchestration, effect/transition contract, interruption and
  playback-evidence semantics, simulated-host test evidence, the
  reachable-state-by-event table, and the PR-2 test-flake correction).
- `voice-oracle-pr2-execution.md` owns VRO-17 PR-2 (hygiene/gating,
  synthesis-only subprocess TTS adapter, engine-gate record,
  no-speaker probe and storage receipts);
  `voice-oracle-pr2-probe.py` (+ results JSON) is its evidence helper.
- `voice-oracle-pr1-execution.md` owns VRO-17 PR-1 (STT adapters,
  failover, partials, D21 provenance amendment, adapter-test receipts,
  native-STT feasibility verdict, real-model probe receipts);
  `voice-oracle-pr1-probe.py` (+ `-probe-results.json`) is the
  real-model probe evidence helper and its receipts (evidence only,
  never production code).
- `voice-oracle-pr0-execution.md` owns VRO-17 PR-0: contract corrections
  (decision record in the PRD), the `vesper-voice` pure core receipts,
  the local-TTS license verification (maintained engine GPL-3.0 — gated
  candidate, not selected), and the NPU readiness record
  (historical device/driver/firmware evidence; userland absence superseded by
  `voice-first-speech-and-npu-assessment.md`).
- `voice-control-execution.md` owns dynamic microphone implementation, long-audio
  lifecycle evidence and explicit real-device/platform acceptance limits.

- `voice-control-recon.md` owns the missing-footer regression diagnosis and
  requested dynamic microphone control implementation boundaries.

## Local Contracts

- Routing JSON/JSONL evidence is LF-pinned by repository attributes so frozen
  corpus and receipt digests remain stable on Windows checkouts.

- The Python source repository is immutable and pinned to `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Distinguish reproduced or locally validated results from CI-pending and product-pending claims.
- Every report records objective, methods, commands, inspected/created files, exact evidence, tests, unresolved issues, platform scope, readiness effect, and status.
- Spike code is evidence only and must not be described as production Agent Vesper implementation.

## Work Guidance

- The VRO-17 continuity/list-marker repair passed Alex's 2026-03-03 listening
  comparison as materially better and good enough for now. Preserve residual
  delay, including the measured first-large-successor cold-start gap, as open;
  do not turn this bounded acceptance into a no-delay or full-VRO claim.
- Update `evidence-index.md` after each bounded phase.
- Use language-neutral fixtures, deterministic local services, isolated state, and secret canaries.

## Verification

- Validate fixture manifests and results against their JSON Schemas.
- Re-run deterministic captures and compare canonical hashes.
- Confirm the source commit and status are invariant at closeout.

## Child DOX Index

No children.
