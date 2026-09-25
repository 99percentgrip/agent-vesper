# Foundation Evidence Index

## VRO-18 xAI host composition (2026-09-25)

- Execution: [`xai-provider-pr7-execution.md`](xai-provider-pr7-execution.md)
- Verdict: PR-7 PASS at offline scope. TUI/ACP registration, authentication
  projection, account model refresh, bounded controls for all six hosted tools,
  shared tool/voice/skill/VRO/memory/worker inheritance and safe citation
  rendering pass affected host/provider suites. Live acceptance and exact-commit
  release gates remain PR-8.

## VRO-18 PR-6 native compaction and WebSocket (2026-09-25)

- Execution: [`xai-provider-pr6-execution.md`](xai-provider-pr6-execution.md)
- Verdict: PR-6 PASS at offline transport and shared-policy scope. Explicit,
  default-off provider-native compaction atomically replaces only the older
  prefix, preserves opaque state, and records quality as unmeasured. xAI
  Global/API-key WebSocket shares HTTP semantics and never replays after
  dispatch; targeted AgentLoop and xAI suites pass. Host controls remain PR-7.

## VRO-18 PR-5 provider-hosted tools (2026-09-25)

- Execution: [`xai-provider-pr5-execution.md`](xai-provider-pr5-execution.md)
- Verdict: PR-5 PASS at offline adapter scope. New provider-neutral hosted-tool
  descriptors/selections keep xAI remote search, code, files, collections and
  Remote MCP separate from Vesper functions. Six applicable tools map only
  after explicit selection in verified Global/API-key mode; unsafe/unknown
  configuration and other auth/region intersections fail closed. Generated
  media stays excluded pending a generic media-output port. Host controls are
  PR-7; no live provider charge occurred.

## VRO-18 PR-4 tools, continuation, usage and citations (2026-09-25)

- Execution: [`xai-provider-pr4-execution.md`](xai-provider-pr4-execution.md)
- Verdict: PR-4 PASS at offline fixture scope. Shared function identities and
  tool results round-trip through the normal AgentLoop; explicit stored
  Responses continuation, bounded prompt-cache routing, opaque reasoning,
  normalized usage and structured citation retention pass 29 tests. The
  adapter remains unregistered; hosted tools, compaction/WebSocket, hosts,
  live acceptance and release remain open.

## VRO-18 PR-3 native Grok session authentication (2026-09-25)

- Execution: [`xai-provider-pr3-execution.md`](xai-provider-pr3-execution.md)
- Verdict: PR-3 PASS at offline loopback scope. Browser OIDC and device
  authorization, refresh/logout, locked Vesper storage, subscription proxy
  routing and strict session/API billing isolation pass 27 targeted tests. The
  adapter remains unregistered; no live provider request occurred.

## VRO-18 PR-2 catalog and capability intersection (2026-09-25)

- Execution: [`xai-provider-pr2-execution.md`](xai-provider-pr2-execution.md)
- Verdict: PR-2 PASS at offline fixture scope. Authenticated language-model
  discovery intersects eight explicit capability records; unknown and retired
  aliases remain non-executable; US endpoint, per-model reasoning, multi-agent
  semantics, image/tool gates and the strict JSON Schema subset are enforced.
  Targeted suite 20/20; hosts and live acceptance remain open.

## VRO-18 PR-1 native API-key Responses transport (2026-09-25)

- Execution: [`xai-provider-pr1-execution.md`](xai-provider-pr1-execution.md)
- Verdict: PR-1 PASS at offline fixture scope. The dedicated xAI crate owns
  API-key credentials, fixed Responses dispatch, strict tool serialization,
  bounded SSE decoding, cancellation, safe errors and opaque reasoning. Red
  strict-schema and oversized-event regressions are green (11/11). The adapter
  is not registered or advertised; PR-2 through PR-8 and live acceptance remain
  open.

## VRO-18 native xAI provider reconnaissance (2026-09-25)

- Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md)
- Architecture reconnaissance: [`../architecture/recon_xai_native_provider.md`](../architecture/recon_xai_native_provider.md)
- Execution report: [`xai-provider-recon-execution.md`](xai-provider-recon-execution.md)
- Baseline: `c64e78f46065f1fbaae899ab9914f3d9b3f7023d`
- First-party source pin: `xai-org/grok-build@f0e3be1100ef5252488e3be8bb0e91cf68d8c305`
- Verdict: PR-0 PASS. Public API-key work may begin. First-party session
  authentication/proxy evidence is sufficient to begin fixture-first PR-3 work,
  while feature availability remains auth-path/model/endpoint intersected and
  fail-closed. No production implementation or live provider call occurred.

- [Critical tool-output stall permanent repair](2026-09-24-critical-tool-output-stall-repair.md) — reproduces the shared `run_command` pipe-backpressure defect red-first, replaces exit-before-read capture with concurrent bounded draining and owned-tree cleanup, verifies TUI/ACP recovery and Linux descendant settlement, and records provider-surface/protocol parity. Its pre-CI OPEN checkpoint is superseded by the five-target closeout below.
- [Tool-output stall five-target platform verification](2026-09-25-tool-output-stall-platform-verification.md) — closes the Windows/macOS behavior gap at exact candidate `27a3f17b…`: all five native jobs and their dedicated nine-case settlement matrices pass in run `36036088105`. Windows fixture quoting/startup defects were repaired red→green. Provider correlation is **NOT EXECUTED — GLM live access unavailable**, retained as an explanatory limitation that does not block release of the classification-D repair.
- [v0.23.6 critical executor-stability release](2026-09-25-v0.23.6-release-execution.md) — **CLOSED — RELEASED:** exact commit `1da9834…` passed canonical `36065369597`, MSRV `36065369716`, five-target `36065369608` and web-driver `36065369575`; release run `36069150695` published 16 assets whose seven sidecars and all GitHub API digests verified. Continuous ACP Registry PR #539 was updated in place to v0.23.6. No local installation was changed.

- [VRO-17 PR-5 host parity and release execution](voice-pr5-host-release-execution.md) — final-phase owning record: ACP truthfully advertises `audio=false` and documents microphone/playback/status/Voice Settings/provider-control exclusions while retaining generic runtime/provider and cancellation parity. A release-feature defect was found: `release.yml` omitted Kokoro/FLM from TUI artifacts; the workflow now compiles the accepted local voice features while keeping model/runtime assets optional and unbundled. Cloud STT/TTS remain future optional adapters through `VoiceStt`/`VoiceTts`. Exact-commit gates, publication, assets/checksums and registry receipt are recorded here as they complete.
- [VRO-17 R6 device acceptance closeout](voice-r6-device-acceptance-closeout.md) — documentation-only closure from Alex's completed real-device matrix: baseline, one-press barge-in, old-speech stop/no-resume, exactly one replacement turn, repeated interruption, explicit Stop with no accidental capture, and later-F9 recovery all pass on the tested setup. Alex clarified that Ctrl+C stopping speech and its tested cancellation message are acceptable. The binding repair's production-entry evidence covers internal synthesis/runtime cancellation; the device run covers observable behavior. Short-reply candidate `f3b736f6…` is separately user-accepted as “Looks like smooth,” and pronunciation remains accepted. Corrected ledger: 17 PASS + 4 scoped + 1 optional/conformant = 22 rows covering R1–R20; PR-5 alone remains open and was not started.
- [VRO-17 short-reply quality and Voice Settings presentation repair](voice-short-reply-quality-repair.md) — traced the occasional short Kokoro pause through the real hygiene→worker path: prior cold/warm evidence ruled out player starvation, while the fixed two-sentence reply measured 0.972 s of stacked model edge padding. Short Kokoro units now retain bounded 50 ms artificial / 100 ms sentence guards (first onset, long units and system speech unchanged), reducing both installed voices to 0.200 s. An initially broad trim was rejected after it exposed 2.15–2.21 s long-form starvation; the narrowed policy restored the paced envelope and exact continuity PTY bytes. Voice Settings now says “Live transcript preview · not supported … (final text still appears after Stop)” instead of implying recognition is unavailable. Red→green tests, combined/default/Kokoro suites, strict Clippy, repository gates, exact-candidate F5/CPU-F9/FLM-NPU-F9 loops, and candidate `f3b736f6…` are recorded. Alex later accepted it as “Looks like smooth” on the tested setup; no numeric, universal or cross-platform acoustic claim. PR-5 was not started.
- [VRO-17 R6 production barge-in / Stop binding repair](voice-r6-binding-repair.md) — closes the §2.4 production binding blocker in source and controlled acceptance: one Speaking+F9 gesture routes directly through the existing genuine `BargeIn` transition (playback stop, synthesis-generation invalidation, one transactional runtime cancel, immediate replacement capture, one bounded interruption context); explicit Stop cancels voice output and runtime without capture; non-voice Ctrl+C remains generic. Red-first production-entry proof, interruption/F5/CPU-F9/FLM-NPU-F9/pronunciation/continuity/R20/R4/two-provider/TUI regressions, Clippy/fmt/architecture/naming/acceptance, and candidate `agent-vesper-tui-r6-binding-repair` (`1b82cc4167ff9333d9de961e078e79a38832b158efa16dbd88c452b257670225`) are recorded. Its implementation-unit verdict was device-acceptance pending; the later device closeout passed. PR-5 was not started.
- [VRO-17 NPU voice user-acceptance closeout — 2026-09-24](voice-npu-user-acceptance-closeout.md) — Documentation-only closeout for Alex's verbatim verdict, *"Cooooollll all working with npu and there is not gaps its sounds natural"*. The tested conversation is accepted for current use; perceived continuity and naturalness are user-confirmed on the tested setup. The future regression baseline is CPU Silero VAD + selected/verified FLM/NPU STT + separately configured reasoning provider + CPU Kokoro TTS. Exact tested executable remains unresolved: no acceptance is attached to `b289a6c1…`, `c4f29171…`, `fa0c3713…`, or the later-built `8895a711…` candidate. No measured-zero-gap, punctuation-pause, recognition-accuracy, interruption, universal, or full-NPU claim; full VRO-17/PR-5/release gates remain separate.
- [VRO-17 Kokoro cold-warm starvation investigation](voice-kokoro-cold-warm-starvation-investigation.md) — Alex's "early short reply pauses, later long speech smooth" observation measured with a fixed-text cold/warm matrix (2 voices × 2 texts × 3 runs, real-rate paced player double, real SpeechWorker→engine→bank→PlaybackOwner chain, plus CPU-load runs): **zero starvation events in every configuration** — the player never waits for data (EOF always beats audio duration by the 64 KiB pipe head). What IS cold: first-piece onset (engine build + ≈1.3 s first-inference warmup; onset RTF 1.24–1.38 cold vs 0.91 warm; steady-state RTF 0.78–0.93). Classification **NOT_REPRODUCED** as playback starvation — no Vesper-owned defect at bank/admission/player/scheduling; no repair applied (depth-2 bank + pipe proven sufficient in measurement). Pronunciation fix verified intact (4/4). Harness retained as `examples/cold_warm_starvation.rs`. Its R6-open statement is historical and superseded by the binding repair plus device PASS.
- [VRO-17 Kokoro pronunciation investigation and bounded repair](voice-kokoro-pronunciation-investigation.md) — Alex's three acoustic defects (reply→"ripple", ready→"read", now→"no") reproduced deterministically through the production phonemizer and root-caused to a **Vesper-owned espeak-ng invocation bug**: the joined text sections were sent to `--stdin` without a terminating newline, so espeak treated the final line as truncated and degraded the last word's pronunciation (diphthong flattening aʊ→oʊ, final-syllable elision ɛdi→iːd, final-consonant drops). Every corruption was final-word-of-piece positioned; mid-sentence words were always correct; segmentation/hygiene/token-mapping/resampling all exonerated by the boundary trace. One-line fix (terminating newline) with red→green proof — including an honestly recorded correction of a vacuous first red attempt (relative-path skip guard). All three words classified PHONEMIZER_ERROR. Candidate `agent-vesper-tui-pronunciation-repair` (3b1bb9d3…). The repair is subsequently user-accepted on the tested path, and no new pronunciation regression was reported in the latest smoothness retest.
- [VRO-17 R6 §2.4 contract reconciliation — historical PRE-ACCEPTANCE BLOCKER](voice-r6-device-interruption-acceptance.md#pre-acceptance-blocker--24-contract-reconciliation-2026-09-24) — the binding contract distinguished BargeIn from StopRequested and correctly found that the pre-repair Speaking+F9 path was speech-mute-only. This historical finding triggered `voice-r6-binding-repair.md`; it is superseded as current status by that repair and Alex's later device PASS. It remains indexed so the rejected two-press interpretation cannot recur.
- [VRO-17 R6 real-device interruption/recovery acceptance](voice-r6-device-interruption-acceptance.md) — **PASS on the tested setup.** A baseline, one-press and repeated barge-in, old-speech stop/no-resume, exactly one replacement turn, explicit Stop with no accidental capture, and later-F9 recovery passed. Alex clarified that Ctrl+C stopping speech and the tested cancellation message were correct. Historical pre-repair briefs and candidates remain in the record but are explicitly superseded. No every-platform/provider, numeric latency, interruption-note audibility or tool-replay claim.
- [VRO-17 R4 optional-partials amendment execution](voice-r4-optional-partials-execution.md) — closes R4 (2026-09-23, Alex-approved Option B): partials amended to an optional STT capability (final-only providers conformant; display-only/bounded/stale-safe/never-submitted when supported); capability table re-verified (all three production STT adapters final-only — no fake FLM partials); capability-aware Voice Settings (`voice_accel::partials_settings_row` + guarded toggle: "unavailable for the selected speech-recognition backend" on every current backend, preference preserved verbatim — red proof by reverting the production row fn); the specified two-fake-provider reasoning-neutrality regression implemented and green (typed + F9 origins × two providers through the same generic path — closing the audit's test gap); PartialGate retained as infrastructure (30/30 unchanged). Candidate `agent-vesper-tui-r4-optional-partials` (673f9fb8…); acceptance 23/23; F5/F9/FLM PTY PASS. Its R6-open statement was accurate at that unit's date; R6 later passed. PR-5 remains open.
- [VRO-17 R4 partials scope decision + provider-neutrality audit](voice-r4-partials-scope-decision.md) — historical decision-only record: it found all current adapters final-only, the generic provider path neutral, and specified the two-provider test gap while recommending Option B. Alex later approved Option B; the R4 amendment execution closed the capability and test gap. The older “awaiting Alex” status applies only to this record's date.
- [VRO-17 R20 default-build capture bounds repair](voice-r20-default-capture-repair.md) — closes R20 (2026-09-23): one managed store owns default F5 and feature F9 captures with the documented caps, reserve, lease recovery and cleanup; red→green default-build and PTY evidence is retained. Its older remaining-open list is historical: R4 and R6 later closed, leaving PR-5 only.
- [VRO-17 final completion audit](voice-vro17-final-completion-audit.md) — evidence-led R1–R20 matrix, phase-gate separation and user-evidence timeline. The 2026-09-24 closeout corrects the current accounting after R4, R20 and R6 closure: 17 PASS + 4 PASS with scoped limitations + 1 optional/conformant = 22 rows covering the 20 requirement IDs. All R1–R20 requirements are closed at approved scope. PR-5 remains the only open phase; the post-v0.23.3 voice tree is still uncommitted and has no exact-commit release evidence.
- [VRO-17 NPU voice — real-device user acceptance](voice-npu-user-acceptance.md) — Alex's 2026-09-23 verdict, verbatim: *"Cooooollll all working with npu and there is not gaps its sounds natural"* — positive real-device acceptance of the tested NPU-enabled conversation experience and perceived speech continuity/naturalness (perceived continuity, not a measured zero-gap claim; ordinary punctuation pauses not claimed removed). Supersedes the earlier "better but still has gaps" verdict as history. Exact tested artifact association **pending** (both live TUIs run an FLM-free installed binary; the exited 10:03/10:09 listening sessions have no launch receipt; root config rewritten at 10:08 is not an in-test record); no candidate basename/checksum is attached by default per Alex's explicit rejection. Composition scope stays as documented: CPU Silero VAD + FLM/NPU recognition (correlation-level placement) + CPU Kokoro synthesis; R16 not globally complete; quantitative latency/accuracy, interruption/cleanup cases, NPU TTS, MSRV/CI and PR-5 remain open.
- [VRO-17 R16 native Verify read failure — diagnosis and repair](voice-verify-read-failure-repair.md) — Alex's user-tested `owned ASR read failed or timed out` reproduced verbatim through the production Settings route; vendor log proof of the mechanism (orphaned owned-shape FLM servers exhaust NPU device contexts; a fresh server's model load fails `DRM_IOCTL_AMDXDNA_CREATE_HWCTX (EINVAL)`, the child dies, and the old read loop mislabeled the resulting reset as a read timeout — 0.879 s, no deadline involved). Repairs, both red→green in the owning `voice_flm.rs`: error-kind classification (`process exited while answering` vs `read timed out`) and a pure-std durable child registry + host-liveness-anchored reaper (dead-host orphan reaped, live-host child proven untouched, no unsafe, no timeout inflation). Candidate `agent-vesper-tui-verify-read-repair` sha256 b289a6c1…; native Verify + F9 PTY PASS against those exact bytes. R16 remains open (natural-speech accuracy, Alex's live-mic acceptance).
- [VRO-17 recording-review continuity boundary repair](voice-continuity-boundary-repair.md) — the 2026-09-23 silent-recording review directive: the zero-capacity handoff pinned the producer through the lane's drain so uncovered slow successor synthesis landed as boundary dead air (red at 1.019 s; depth-1 insufficiency derived, not assumed); smallest correction is the bounded depth-2 bank in the existing worker. Also repaired en route, both with red/green evidence: the FLM session's silent drop of `VESPER_PYTHON_PATH` precedence in the conversation-CPU adapter (caught only by the review-mandated legacy PTY loop; preserved older candidates PASS, the FLM candidate FAILS) and the scope-blind "using CPU" readiness conflation (now CPU-selected/available-not-selected/unavailable-refusal wording plus a real last-request route receipt). Test-isolation defects fixed: process-global chdir races (parity + flm_route roots, now parallel-stable) and verification-state order pollution. Candidate `agent-vesper-tui-continuity-boundary-repair` sha256 fa0c3713…; Alex's acoustic listening acceptance remains the open boundary.
- [VRO-17 NPU STT implementation gate — shared Silero VAD and FLM launch-gate correction](voice-npu-stt-implementation-progress.md) — persistent VAD protocol/regression on both silence and speech-positive paths (one speech-path defect found and fixed), explicitly authorized/hash-verified FLM Whisper asset, the initially failed launch receipts, and the 2026-09-22 corrected standalone experiment: FLM 1.0.5 `serve --asr 1` serves real NPU transcription on loopback (backend evidence; no production wiring yet; VAD preprocessor proven load-bearing because the backend hallucinates on unfiltered silence).
- [VRO-17 R16 production F9 routing repair](voice-npu-stt-implementation-progress.md#production-f9-routing-repair-2026-09-22-second-session) — the PTY round-trip exposed that F9's stop path ran the CPU sidecar while the FLM adapter was never given audio; fixed via the `voice.rs` conversation-STT slot, with three consecutive end-to-end passes (Settings save → Verify → F9 → FLM adapter → one agent turn → Kokoro PCM at a no-device sink; CPU recognizer poisoned and never imported) and all voice gates re-run.
[Voice pause audit — three culprits identified](voice-pause-audit.md): offline reproduction with the installed Kokoro pack shows ~400 ms leading/~770 ms trailing silence per synthesized piece stacking into 1.2 s gaps at piece boundaries, em-dash/hyphen punctuation tokens inserting model-level breaks ("Confirmed —" 380 ms vs 130 ms plain), and pre-speech compute latency as the third contributor. No code changes; fixes left for a scoped repair decision.
[Voice latency/performance repair after native acceptance](voice-latency-performance-repair.md): candidate uses a 28-character first/48-character successor policy after hygiene and one reusable worker per Natural Voice pack screen. The fixed long-unit fixture reached first PCM in 1.62–2.59 s across observed post-repair runs; release-mode ready-worker Preview attempts reached fixture PCM in 1.559/1.569 s. Focused tests and strict Clippy pass. Alex ran the release candidate and reported “ok its way faster then before.” The latency repair is implemented and qualitatively user-confirmed; repeat timings and the remaining full-phase gates stay open.
[Voice latency user acceptance — initial retest](voice-user-latency-acceptance.md): **NOT ACCEPTED / PHASE OPEN.** Alex observed approximately 20 s, 40 s and 20 s to first spoken output across three attempts and approximately 10–15 s for Preview; recording was described as fast (~2–3 s on the first attempt). A later locally verified candidate is linked above but has not received native user acceptance. These human observations override any phase-completion inference from narrow fixture benchmarks.
[First-speech repair and native NPU assessment](voice-first-speech-and-npu-assessment.md): code-level local repair only. One authorized live turn reached first PCM at 26.99 s before the repair; a subsequent local fixed-text benchmark measured 8.602 s whole-unit preparation versus 2.733 s first PCM, with one user-confirmed natural playback. The later native retest above failed latency acceptance. AMD NPU/FLM/XRT readiness is verified, but native STT and TTS offload gates remain separately open.
[Speech sentence-pipeline repair](voice-speech-pipeline-repair.md): one-unit lookahead removes the reproduced serial synthesis/drain gap; isolated Stop/FIFO regressions pass and Alex confirmed the overlapping check had no sentence delay. The initial live coding reply delay remains unresolved.
[Voice latency repair](voice-latency-repair.md): repeated pack checks reduced from about 1500 ms to below 0.11 ms; slow-import recorder regression and direct/VRO first-PCM timings recorded. No instant/live-provider latency claim.
[Kokoro PCM scaling repair](voice-kokoro-pcm-scaling-repair.md): missing float-to-s16 amplitude scaling reproduced and repaired; both real voices and direct/VRO signal gates pass; Alex confirmed one corrected playback clear and complete. Broader native acceptance remains open.
[Authorized Kokoro playback probe](voice-kokoro-authorized-playback.md): real worker and player drained the original approved sample, but Alex confirmed he did not hear it. The later PCM repair and separately approved corrected listening result are linked above.
[Voice playback diagnostics repair](voice-playback-diagnostics-repair.md): device-free red→green failure classification and stderr-pressure tests; the later PCM scaling report above identifies and repairs the silent-signal defect.
[Voice-oracle reconnaissance](../architecture/recon_voice_oracle.md): read-only, line-cited analysis of the pinned upstream (`88998de`, MIT) — STT failover with unavailable≠error, sentence-gated streaming TTS with pre-cloud secret redaction, barge-in that records the last heard sentence and prefixes the next turn, per-stage latency telemetry on every outcome; includes the VAD-policy conflict with our shipped evidence and the documented-but-unimplemented `ack` gap. Work-unit record: [voice-oracle-recon-execution](voice-oracle-recon-execution.md) (no implementation; directive-authorized source reading recorded as a deviation in `docs/architecture/AGENTS.md`). Feeds [VRO-17 — the voice-oracle extraction PRD](../voice-oracle-extraction-prd.md): `vesper-voice` provider-neutral core, interchangeable local/cloud STT/TTS ports, agent-free orchestration over the existing runtime cancellation path, and the PR-0…PR-5 gated migration plan.
[VRO-17 PR-0 execution](voice-oracle-pr0-execution.md): contract hardening (20 findings D1–D20, incl. the GPL-3.0 refutation of the "Piper (MIT)" dependency assumption and the NPU device-present/userland-absent record) and the `vesper-voice` pure core — 61 crate tests, workspace 2528/0, arch 29 pkgs, naming 36 frozen, acceptance 23/23; no adapters/capture/hosts touched; PR-1 ready against frozen contracts. Updated PRD: decision record §7 + traceability matrix §8.
[VRO-17 PR-1 execution](voice-oracle-pr1-execution.md): `stt-sidecar` (Rust adapter + shipped sidecar inference, warm child, process-group reaping, bounded IO, VAD binding), `stt-http` (validated endpoint, redirect refusal, truthful error matrix, D21 `LegacyEmptyResponse` provenance for legacy-worker empties), `FailoverStt` (frozen policy, per-attempt egress recheck, attempt bound), `PartialGate` (buffered-repass partials, whole-capture budget, final priority, stale rejection, no turn-submission surface), 30 adapter tests over subprocess/TCP fixtures; workspace 2569/0 all-features, arch 29, naming 36 frozen, acceptance 23/23, dictation suites untouched. Real-model probes (cached tiny/base, VAD on): silence 30/90 s and tone fixtures all empty — no hallucinations; **speech-fixture recognition receipt remains open**. Native-STT verdict: suitable-with-prerequisites, nothing implemented. PRD amended: D21/D22 + status/PR-1 sections.
[Existing voice-stack reconnaissance](../architecture/recon_existing_voice_stack.md): read-only trace of the shipped v0.23.3 push-to-talk path (F5 → arecord/afrecord → shipped sidecar, `vad_filter=True` → composer append, never auto-submit) and the auditable negative result for assistant speech output (bell-only; no TTS module/dep/call site in any build; PR-0/PR-1 unreferenced by hosts). Reuse-vs-gap matrix separates synthesis/playback/dispatch/hygiene/interruption/connection gaps; recommendation-only PR-2 correction: smallest task is a feature-gated subprocess TTS adapter over user-installed system speech binaries — **not reviewed by Alex yet; PR-2 remains paused**.
[VRO-17 PR-2 execution](voice-oracle-pr2-execution.md): hygiene/sentence gate (cross-chunk protected spans, redaction, bounded overflow, 16 tests) + synthesis-only `tts-subprocess` adapter (engine gate PASS as optional user-installed system-engine baseline — no bundling, zero new deps, `deny.toml` untouched; streaming-WAV validation ignoring placeholder sizes, 22050→16000 conversion, no audio files by construction, 19 tests incl. storage-safety suite). Real no-speaker probe: mono 22050 Hz s16 WAV, ~6 ms first output, artifacts cleaned (334 KiB peak / 0 residual). Storage receipt inside; workspace all-features 2604/0, acceptance 23/23, dictation 8/8. Open: R3 (cloud + quality), PR-1 speech receipt, R20 PR-4 capture items. Not a speaking TUI.
[VRO-17 PR-3 execution](voice-oracle-pr3-execution.md): `VoiceSession` orchestration — typed effects (exactly-one submit, transactional cancel, urgent-ordered speech stops), five-dimension overlap, D21-provenance submission gating, hygiene fed exactly once, pre-identity cancellation applied to the matching late run, idempotent Stop, bounded pending input, validated playback receipts with one-time honest interruption notes, metadata-only reports; 29 tests (four scenario families) against the production session with simulated hosts; reachable-state-by-event table recorded. Also: PR-2 storage-test flake root-caused and fixed (test-isolation defect, adapter zero-file property never violated). Workspace all-features 2633/0 (×3 stable), acceptance 23/23, dictation 8/8. Open: R3, speech receipt, R20, CI targets. Not a speaking TUI.
[VRO-17 PR-4 execution](voice-oracle-pr4-execution.md): opt-in `voice-conversation` TUI feature — ConversationController on the real seams, PlaybackOwner (bounded queue, verified receipt semantics, no files), R20 capture store (hard caps, cross-instance lease reservation, dead-lease recovery), F9/Settings activation, PR-3 note wording corrected. **Synthetic fixture STT gate closed 6/6** (pre-selected criterion; first-attempt miss recorded). TUI default suite 395/0 unchanged (R9); workspace all-features 2657/0; acceptance 23/23. **Device acceptance user-operated and OPEN** — not advertised as working device voice. Build/launch instructions + checklist in §8.
[VRO-17 PR-4 acceptance-readiness addendum](voice-oracle-pr4-execution.md#pr-4-acceptance-readiness-addendum-this-unit): pre-device verification found the previously reported binary **stale** (rebuilt; incremental cost 0 B) and — the material finding — a **demonstrated wiring gap**: F9 gates and shares capture, but the conversation controller/session is constructed only in tests (linker-GC proof: session/playback/store symbols absent from the live path), and no Settings → Voice panel exists (config-file is the only activation route). Conversation checklist items 2–3 **blocked** pending a small, focused binding; dictation regression ready. R9 re-proven at the right boundary (default 395/0; feature-enabled unconfigured constructs nothing; new targeted test; TUI voice-feature 420/0; workspace all-features 2658/0). Storage wording corrected (active capture creates bounded temp files; residual-after-cleanup is the zero-target claim).
[VRO-17 PR-4 binding repair](voice-oracle-pr4-repair.md): both addendum blockers fixed and red→green proven at the production entry points — Settings → Voice panel (palette entry, activation/partials/readiness, persisted through the existing Save flow, immediately effective on F9), lazy `ConversationHost` constructed only on an enabled gesture, origin-tagged transcript routing (F5 composer / F9 conversation final → provenance-gated `submit_stt_final` → `spawn_submitted_prompt` typed-Enter path), ContentDelta→hygiene→TTS→PlaybackOwner with speech-failure isolation, settlement/identity bridges, stop = playback-flush-first, one shared sidecar, and R20 bound to the recorder in feature builds (default-build legacy path recorded as an explicit uncovered gap). `pr4_wiring` 7/7 red→green; TUI default 395/0 (R9), voice-feature 427/0, workspace all-features 2665/0, acceptance 23/23. Artifact sha256 e57eb23d… (identity recorded). **Device acceptance still open (Alex-operated).**
[PR-4 Voice-save blocker fixed](voice-oracle-pr4-repair.md#addendum-voice-settings-save-flow-blocker-alexs-device-test): Alex's user test exposed two one-line defects — the voice save was nested under the web-tools change condition, and voice was absent from the Settings dirty check (so Esc skipped the Save prompt entirely for voice-only changes). Both fixed via a single shared `settings_voice_save` implementation (`voice_dirty` / `voice_save_required`), watched-path added, live-host scope sync on save, failed-save error retained. Red→green proven by reintroducing the defects; pr4_wiring 9/9; TUI default 395/0, voice-feature 429/0, workspace all-features 2667/0, acceptance 23/23. Artifact sha256 a2e3b029…. **Device acceptance still open (Alex-operated retry).**
[PR-4 F9-refusal root-caused & fixed](voice-oracle-pr4-repair.md#addendum-2-f9-refusal-with-saved-on-second-device-test): the gate conflated enabled+readiness AND its engine check `PathBuf::from("espeak-ng").is_file()` resolved against the process CWD — always false in the workspace while `/usr/bin/espeak-ng` sat on PATH (Settings' panel had the same bug). New shared `voice_readiness` module (PATH-resolved existence, named checks+remedies); F9 now separates disabled vs enabled-but-blocked with accurate messages; panel and gate use ONE assessment. Red→green proven against the exact defect; `pr4_f9_gate` 6/6; voice-feature 435/0 ×3; workspace 2673/0; artifact sha256 ebf52b83…. **Device acceptance open (Alex retry).**
[PR-4 second blocker fixed](voice-oracle-pr4-repair.md#addendum-3-the-real-blocker--a-stale-duplicate-enablement-flag): Alex's third test (readiness all-yes but F9 refused) pinned the true cause — `ConversationHost` carried a second `scope_enabled` flag defaulting to false that no production caller ever set (the save-time sync only ran when the host already existed, but the host is lazily created after save). Flag removed; single enablement authority = the F9 gate. Binary verified (stale refusal gone, gate + blocked messages present). pr4_wiring 10/10 incl. gate→host→submit end-to-end; workspace 2674/0; artifact sha256 c34e5df1…. **Device acceptance open (Alex retry; failures now name the actual prerequisite).**
[PR-4 artifact-clobber note](voice-oracle-pr4-repair.md#addendum-4-feature-removed--artifact-clobbered-by-a-default-build): Alex's "you removed the feature" was a default `cargo build` overwriting the feature build at the same path (byte-verified: voice symbols absent, then restored byte-identical c34e5df1…). No code change. Always build with `--features voice-conversation`.
[✅ PR-4 DEVICE ACCEPTANCE PASSED](voice-oracle-pr4-repair.md#device-acceptance--passed-alex-first-live-round-trip): Alex's first live round trip through the production path — spoken no-tool instruction recognized (surfaced as "you (voice): …"), agent ran 0 tools as instructed, text answer produced, audible voice reply observed. Core bidirectional loop closed on-device. Open: live barge-in exercise (optional checklist), PR-5, R3 voice choice, CI targets.
[PR-4 live-use defect fixed: speech died after first barge-in](voice-oracle-pr4-repair.md#addendum-5-live-session-defect--speech-died-permanently-after-the-first-barge-in): Alex's multi-turn session isolated it — `stop_flush` latched `stopped=true` and nothing ever called `reset()`, so every later `begin_stream` silently swallowed audio. Fix: a new stream clears the latch (canceled-stream suppression unchanged, separately tested); regression `new_stream_after_stop_is_audible_again` red→green; playback 10/10; artifact 5d8aa9e8…. **Awaiting Alex's multi-turn+barge-in retry.**
[PR-4 FINAL REPORT — device acceptance passed, live defect fixed](voice-oracle-pr4-repair.md#final-report--pr-4-complete-all-phases-through-device-acceptance--live-defect-fix): consolidates the full phase ledger (recon → PR-0..3 → implementation → two binding repairs → device acceptance → live stop-latch fix) with the final audit: all gates re-verified on the current tree (TUI 395/0 default + 437/0 voice-feature, voice 166/0, dictation 8/8, workspace 2675/0 all-features, fmt/Clippy/arch/naming/acceptance clean), audit findings 1–5 (stale flag, CWD-relative check, stop latch — each red→green proven; waiver narrowing; overstatement corrected in place), honest scope statement, storage delta. **PR-4 CLOSED.** PR-5 open.
[Kokoro-82M local neural voice audition](voice-oracle-local-neural-audition.md): feasibility + listening choices + exact asset budget (~87 MiB) for af_heart/am_michael. Recommended route: ONNX q8f16 + `ort` 1.88 (license-clean, MSRV match) with espeak-ng `--ipa` phonemizer (100% vocab coverage verified). Creator samples verified accessible (af_heart 6 samples; am_michael creator samples absent — CDN sample noted); demo Space verified real (hexgrad). Local probe NOT run (no assets). Integration proposal outlined, not implemented. **Awaiting Alex: listening + budget approval.**
[VRO-17 R3 implementation — Natural Voice pack](voice-oracle-kokoro-implementation.md): the approved Kokoro upgrade landed as an optional, user-installable pack — new dedicated adapter crate `vesper-voice-kokoro` (pack descriptor/integrity/lifecycle, espeak-IPA pronunciation bridge reimplemented from the official pipeline's algorithm, bounded ORT engine, `VoiceTts` adapter, dev-only mock) behind the new default-off TUI feature `voice-kokoro`; Settings → Voice gains engine selection + Install (confirmed dialog with real numbers, real byte progress, Esc-stop, backup/restore, resume), Preview (real adapter + PlaybackOwner, fixed phrase, never a turn), Repair/Verify, measured ownership-safe Remove, and Details (licenses/provenance/location); F9 gate joins the pack+phonemizer checks into the SAME shared assessment (enabled-but-blocked names the prerequisite; no silent fallback). Real setup receipt: production pipeline into the managed per-user cache in 24.3 s, silent synthesis probe passed, Ready published only after probe; retained 117 677 941 B measured (< 256 MiB), peak 120 802 497 B (< 512 MiB). Performance (release, real inference): audio/synthesis ≈ 1.5× (audit correction: this implies conventional RTF ≈ 0.67, not slower-than-real-time), cold init 319 ms, peak RSS 815 MiB, engine usable after cancellation. **Recorded decision: q8f16 SIGSEGVs ORT 1.28.0 CPU → pinned `model_quantized.onnx`** (directive-preauthorized alternative, +6.3 MiB). Gates: fmt/Clippy strict/arch 30 pkgs/naming 36 frozen/acceptance 23/23 clean; workspace all-features 2720/0, default 2505/0; artifact `44e3d8da…` feature build (capability byte-verified; installed binaries untouched). **Open: Alex's listening/F9 acceptance; PR-5.**
[VRO-17 continuity & formatting repair](voice-continuity-phoneme-repair.md): implements the two recon-proposed repairs. Subdivision superseded: the 28/48 micro-cut policy is replaced by clause-sized first piece (onset preserved: genuine clause boundary preferred, honest word fallback) + sentence-level successors within the 510-ID context budget — measured on the repaired path: 5 pieces (was 12), stacked fade-quiet 2.9 s (was 7.2 s), artificial mid-sentence insertions 11→1, agg-RTF unchanged (0.66), paced sink gap-free except ONE quantified 5.2 s cold-start gap at the first big successor (bounded audio-duration prebuffer proposed separately for review, NOT slipped in). Formatting: the hygiene gate now keeps list markers attached to their items (`leading_list_marker` + marker-aware `find_sentence_end` + prefix deferral + `formatting-only` marker at finalize) — the exact device reproducer (`"1."` unit → zero phoneme ids) is fixed red→green through the full PTY production path with word-sized streamed deltas; mandatory distinctions pinned (headings keep their word, `The answer is 1.`/decimals/versions never lose content, symbols-as-content still emit, meaningful zero-phoneme failures stay visible). Open finding recorded: pre-existing decimal-split-at-chunk-boundary limitation. New suite `hygiene_formatting_units` (13, every-character-boundary) + pipeline fixture updated to the superseding policy with onset protection kept. Candidate `agent-vesper-tui-continuity-phoneme-repair` sha256 9d59189a…; prior candidates preserved. Gates: all affected suites + 4 PTY modes + clippy(3)/arch 30/naming 36/acceptance 23/fmt-scoped green. **Alex's 2026-03-03 listening comparison PASSED for this bounded repair: delay remains, but it is “much better then before” and “good enough for now”; residual delay remains open and the continuity verdict stays distinct from software receipts.**
[VRO-17 real NPU speech integration reconnaissance](voice-npu-integration-execution.md): current-machine/runtime and installed-version source audit. FLM 1.0.5 has a concrete standalone Whisper NPU endpoint, but its handler has no verified VAD/silence contract and ignores request cancellation; Vesper's VAD is backend-local, so registration would violate R16 or instantiate the CPU recognizer as a fake NPU result. The ~1 GB model is absent and requires itemized native consent. Kokoro Linux-NPU compatibility remains unestablished. No production code, download, process, candidate, or installed binary changed.
[VRO-17 continuity listening acceptance](voice-continuity-listening-acceptance.md): device closeout for the preserved candidate, including Alex's exact verdict, scope-limited acceptance, root causes, red→green receipts, measured continuity/onset tradeoff, commands, deviations, and explicit residuals. No production code, installer, or installed binary changed.
[VRO-17 continuity & empty-phoneme recon](voice-continuity-phoneme-recon.md): measured reconnaissance after Alex's device feedback (speech restored; long replies pause excessively; some output fails `no pronounceable phonemes`). Pause mechanism quantified on the real engine with a paced sink: every inference piece carries 0.25–0.46 s fade-in/out quiet runs that CONCATENATE at the 28/48 mid-sentence boundaries (7.2 s stacked silence in a 41 s passage; whole-sentence pieces stack 2.9 s at natural pauses); warm RTF 0.63–0.66 sustains pace while cold-start RTF 0.96–0.99 leaves no margin. Empty-phoneme root cause reproduced: numbered-list markers emit `"1."` as a hygiene unit that maps to zero vocab ids (22 symbol chars are zero-id alone; harmless embedded). Minimal repairs proposed (both now implemented by the repair record above). Earlier EPIPE wording corrected (errno = no read end remains; reaping distinction from the paired experiment).
[VRO-17 multi-turn playback repair](voice-multiturn-playback-repair.md): Alex's real-device result recorded as FAILED for repeated-turn playback/continuity (turns 1–2 spoke; from ~turn 3 sentences stopped finishing with repeated `player pipe write failed (device closed?)` under both CPU and Automatic — the registry is empty, so both ran CPU). Cause established layer by layer: EPIPE proves child death (write-after-reap fails; write-after-unreaped-SIGTERM still succeeds into the 64 KiB buffer), the production aplay argv carried `--fatal-errors` whose documented behavior aborts on any recoverable xrun — turning per-piece starvation gaps into mid-stream child death — and the per-segment stream shape explained the warning storm (one player per hygiene sentence; in-segment short-circuiting was already correct). Repairs, red→green (`voice_multiturn_playback`, 11 tests incl. ten consecutive turns in one process, turn-3 death failing once with turns 4–6 recovering, short-consumer honesty, errno preservation): removed `--fatal-errors`, preserved the original io error (kind+OS code) with truthful consequence text, contained dead streams (fail-fast remaining pieces; fresh child next turn), made exit-0-before-stdin-close a failure, fixed appended-stream byte accounting. Candidate `target/voice-candidates/agent-vesper-tui-multiturn-repair` sha256 7bd31168…; prior candidate preserved. STT garbling recorded OPEN (faster-whisper base/int8 CPU; `vad_filter=True` pinned; no forced language ⇒ per-chunk auto-detect — hypotheses only, no STT change). Gates: new suite 11/11, voice suites + PTY (direct/VRO/Preview) green, clippy/arch/naming/acceptance green, scoped fmt clean; MSRV/CI/cargo-deny not run. **Real-device retest of the repair showed speech restored; continuity/phoneme issues now owned by the recon record above. No acoustic or NPU claims.**
[VRO-17 CPU production acceptance](voice-cpu-production-acceptance.md): the running CPU voice application verified through its real event loop — PTY loop regression in all three modes (direct, VRO, Settings Preview) with isolation pre-verified (loopback-only provider, deterministic recorder/STT/player doubles, hard-linked verified pack), and three defects repaired red→green: Preview ignored the TTS execution policy (now resolves through the same shared rule as F9 via `voice_accel::preview_policy_gate`), a no-op Settings save rebuilt the acoustic engine (unchanged selection now sends no Replace), and a pack-screen action off-by-one made the Preview row fire Repair (rows/indexes now derive from one ordered action list; caught BY the production-loop check, not by unit tests alone). New suites `voice_policy_parity` (7) and `voice_interruption_lifecycle` (5: five interruption cycles then full-turn recovery, no transcript replay, failure-before-audio recovery, session-exit clean lane, Preview worker lifetime). Release-profile application timings recorded stage-distinct (Preview click-to-first-PCM 1.75–2.87 s PTY; warm worker 1.56–2.49 s; F9 stop-to-first-PCM 2.44–3.18 s incl. synthetic STT/provider holds; long-unit 1.7–2.7 s). Preserved candidate `target/voice-candidates/agent-vesper-tui-voice-cpu-acceptance` sha256 d633b4c3…, identity re-verified. Gates: clippy -D warnings (3 feature sets), architecture 28, naming 33, acceptance 23/23, scoped fmt clean, docs links/whitespace clean; MSRV/cross-target/CI/cargo-deny not run. Its single-turn software receipts stand with their stated scope; sustained real-device playback failed afterward and is owned by the repair record above.
[VRO-17 capability-gated execution](voice-capability-gated-execution.md): the clarified R16 direction implemented as production selection policy — pure-core stage execution policy (CPU / Automatic-verified-only / strict-NPU per stage; ten evidence-based readiness facts; in-process readiness cache with invalidation; honest offload attribution), the honest empty accelerator registry (no NPU route registered/offered), F9 gate stage-policy resolution, Settings → Voice compute menus, and `[voice]` scope persistence (`stt_compute`/`tts_compute`, default `cpu`). Ten production-path regressions prove the no-NPU matrix incl. zero-accelerator-call CPU policy, ordinary-CPU Automatic, stage independence, stale-strict revalidation, and no fabricated placement success. CPU candidate preserved (Preview first-PCM 1.684–1.693 s; long-unit 1.674 s fixture receipts). Gates: fmt (scoped)/Clippy/arch 28/naming 33/acceptance 23/23; vesper-voice 85+42, TUI voice 278 lib + 10 policy + 20 pr4/r3 + 9 pipeline; default TUI lib 242 (R9 parity). Artifact d86389a3… capability-verified. **NPU STT/TTS remain open unimplemented gates; no device/PR-5 claims.**
[Execution report and audit](voice-oracle-kokoro-implementation.md#13-production-routing-repair-and-audit):

### VRO-17 R3 — production-routing repair (current acceptance remains open)

[Execution report and audit](voice-oracle-kokoro-implementation.md#13-production-routing-repair-and-audit):
F9-scoped host speech instructions, terminal-only answer routing, enqueue-time
cancellation identity, truthful player errors, bounded worker admission, and
nonblocking Stop ownership. Removed the production reply-injection shortcut;
the real binary now passes loopback provider-wire + real Kokoro tests for
Michael/direct and Heart/VRO, with microphone/player doubles and baseline speech
rejected. Local feature build produced; no device, live-provider or release
acceptance claimed. Earlier implementation-complete language is withdrawn.


[Vesper Bridge Increment 15](../Vesper%20bridge/recon/phase2p-portal-resolve-enrollment.md)
[Full-Mission Audit 0→2p](../Vesper%20bridge/recon/full-mission-audit.md)
[Increment 16 — audit fixes](../Vesper%20bridge/recon/phase2q-audit-fixes-report.md)
[Increment 17 — Resolve free 21.1 live probes](../Vesper%20bridge/recon/phase2r-resolve-live-probes.md): external scripting Studio-gated (proven), internal Console route proven live (`VB3|21.1.0.17|…`), `resolve --version` hang root-caused.
[Increment 18 — Resolve vertical slice VERIFIED](../Vesper%20bridge/recon/phase2s-resolve-vertical-slice.md): import → timeline → render → **output file frame-exact verified (2245/2245, full decode RC=0)**, original sha256 unchanged; free-edition codec/import defects measured and fixed (no H.264 in free; ImportMedia string form; CustomName breaks AddRenderJob).
[Increment 19 — paste-free autonomous worker](../Vesper%20bridge/recon/phase2t-paste-free-worker.md): **one paste per launch, then fully autonomous** file-driven control via LuaJIT-FFI channel (`ffi.C.fopen`); second render executed zero-human and frame-verified 2245/2245; `%w`-excludes-underscore and pattern bugs found live.
[Increment 20 — Elisa via MPRIS: second app PASS](../Vesper%20bridge/recon/phase2u-elisa-mpris.md): **40 seconds, zero per-app code** — play verified at 3 layers (state/position/PipeWire stream); the ladder claim measured against Resolve's 4-hour archaeology.
[Increment 21 — native adapter integration](../Vesper%20bridge/recon/phase2v-native-adapter-integration.md): `AdapterPort` seam + Resolve file-IPC and MPRIS adapters behind the **real** `bridge_execute` authorization path — **live-tested against both running applications**; adapters are dependency-free (busctl/file only).
[Increment 22 — §7 dispatch-integrity hardening](../Vesper%20bridge/recon/phase2w-dispatch-hardening.md): timeout-after-mutation now settles `unknown_outcome` (never `failed`); **duplicate suppression re-keyed from attempt-id to operation identity** (the old key could never catch a real retry); step-5a semantic suppression precedes freshness. Live re-verified on both apps.
[Increment 23 — latency investigation](../Vesper%20bridge/recon/phase2x-latency-investigation.md): reported latency root-caused to a **10s test-ceiling window × my full-suite-between-steps procedure**, not the product — Bridge path measured **22 ms** end-to-end; ceiling override 10s→1s (suite 10.0s→1.3s); live latency-budget test (<1s) added as regression guard.
[Increment 24 — three-track choreography](../Vesper%20bridge/recon/phase2y-three-track-choreography.md): 30s/15s/30s three-phase sequenced control PASS (±274 ms); **Elisa crash under rapid Next/Previous inversion found** — app defect, spaced-calls mitigation recorded for adapters.
[Increment 25 — measured-behavior adapter guards](../Vesper%20bridge/recon/phase2z-adapter-behavior-guards.md): adapter refuses previous-while-paused (restart trap), enforces a 1200ms inversion cooldown (crash trigger), and settles navigation only on **trackid change** — all three guards tested against a scripted bus fake and re-verified live.
[Increment 26 — audio-event stop](../Vesper%20bridge/recon/phase2aa-audio-event-stop.md): kick-drum onset **located from the waveform** (low-band transient analysis: 59.28s, the drop); played 30s→59.5s, stopped +216ms past onset, app closed; MPRIS **Seek-is-relative** semantics discovered and recorded.
[Increment 27 — first-kick correction](../Vesper%20bridge/recon/phase2ab-first-kick-correction.md): Alex's ground truth (kick at **51s**, continuous after) confirmed by re-analysis at the right threshold — my "59s drop" was a narrative over incomplete data; test re-run, stopped at 51.9s. Lesson: detectors report structure + uncertainty; human ground truth re-examines, never gets overridden.
[Increment 28 — wave detector calibrated](../Vesper%20bridge/recon/phase2ad-wave-detector.md): `tools/bridge/kick_detector.py` — first sustained kick entry from waveform alone; **5 pipeline stages each earned by a measured failure**; validated blind on two artist-confirmed tracks (51.55 vs 51.5; 56.46 vs 56.0); FX fills (run≤3) vs kick entries (run≥4) discriminator.
[Increment 29 — FX-fill choreography](../Vesper%20bridge/recon/phase2ae-fx-fill-choreography.md): FX fill located at **27.88s** (sharpness 11.7); interleaved beat/half-time patterns disambiguated with one human question; 4th kick = **59.11s**; played from the fill, stopped on the 4th (+428 ms).
[Increment 30 — repeat choreography + close](../Vesper%20bridge/recon/phase2af-fx-fill-repeat.md): full user-level task end-to-end — both waveform facts **independently re-derived** (no cached values), 4th-kick stop at **+98 ms** (tightest yet), Elisa closed and verified gone.
[Increment 31 — video edit delivered](../Vesper%20bridge/recon/phase2ag-video-edit-delivery.md): Suno banner **blurred 175–185s (edge-energy verified 10× collapse)**, LinkedIn MP4 (1080p30, 172 MB) produced, original sha256 untouched; six Resolve-Fusion free-edition findings recorded; blur executed via ffmpeg after the scripted Fusion route proved unrenderable.
[Increment 32 — v2 full-panel fix](../Vesper%20bridge/recon/phase2ah-video-edit-v2.md): v1 rejection accepted (region missed lower rows; coverage gaps) — full panel square blurred 172–188s (Alex-confirmed only visible window), verified unreadable on the delivered file, original intact.
[Increment 33 — v3 full-height blur](../Vesper%20bridge/recon/phase2ah-video-edit-v3.md): Alex's screenshot showed text below AND above the v2 box — row-profile scan measured the panel at y 0.04–0.93; v3 blurs the full window height 174–187s, verified 0.04–0.24 edge inside vs 1.1–3.8 outside. Worker-loop Console blocking, item-query and timeline-shell behaviors recorded.
[Increment 34 — v4 tight text-box](../Vesper%20bridge/recon/phase2ah-video-edit-v4.md): "Too big!" — box re-sized to the text block only (x 460–1573, y 378–831); inside 0.11 vs outside 1.4–5.7 edge energy; outside content sharp, full decode clean.
[Increment 35 — /usage regression root-caused](../Vesper%20bridge/recon/phase2ai-usage-regression.md): live-wire evidence — Z.ai account exhausted (chat 429 `1113`), monitor answers HTTP 200 error-envelope; client bug fixed red-first (4/4) so the panel now says "Z.ai quota monitor reported an account error" instead of blaming malformed protocol; PTY loop on the real release binary PASS; plan recharge is Alex's manual action.
[Increment 36 — Manor Lords input fixed, flow documented](../Vesper%20bridge/recon/phase2aj-manor-lords-status.md): menu-clicking root cause found — XWayland pointer mapping ×1.4 + `mousemove --sync` hangs forever (never use); clicks + spectacle observation proven, full tutorial video decoded frame-by-frame (dev-popup Continue, C=construction NOT B); reached in-game HUD twice; village build BLOCKED on identifying one setup screen — Alex's one-word answer unblocks.
[Increment 36 — Manor Lords mission](../Vesper%20bridge/recon/phase2aj-manor-lords.md): menu-clicking **FIXED** (xdotool `--sync` hangs on XWayland; HiDPI real=logical×1.4 mapping measured; keys reach game) — launched, New Game, region+START, dev-popup Continue all navigated live; docs read properly (C=construction confirmed from keycap glyphs; Alex's correction verified); **village placement BLOCKED** by raw-input in-world cursor (needs ydotool/uinput); game exited via accidental Quit — no new save on disk, recorded honestly.
[Increment 37 — input injection engineered](../Vesper%20bridge/recon/phase2ak-input-injection.md): `vesper_pointer.py` builds full uinput stack userland (correct `input_event` `<qqHHi` layout, UI_DEV_SETUP API); devices register and KWin lists them as pointers — **events dropped at compositor seat arbitration** (measured, with journal evidence); 5 injection routes tested and mapped; game relaunched, menu control re-verified; single unblock action recorded (sudo install ydotoold).
— portal probe **executed under Alex's approval: PASS at OS level** (4
real 2880×1800 captures via the xdg Screenshot portal) — but the earlier
hypothesis "portal unblocks Cua capture" is **refuted by measurement**:
0.28.1's wayland-helper is GNOME-Shell-only and upstream states KDE
needs a KWin activation adapter "not yet provided" (libei input is
focus-global). Certifiable lanes listed: X11/GNOME session or the KWin
adapter; none claimed supported. Resolve **free 21.1 confirmed to exist
for Linux** (API: `davinci-resolve` 21.1.014) but the zip sits behind a
JS registration flow — probed page/bundle/API/CDN, no bypass used, two
manual options offered. **Enrollment fixed and ARMED** to the original
PRD (234 ≤ 512 ceiling); takes effect on Alex's TUI restart — no
Settings typing needed.

[Vesper Bridge Increment 14](../Vesper%20bridge/recon/phase2o-installs-report.md)
— installs executed under Alex's explicit authorization: repaired
Vesper binaries installed (installed bytes verified to carry the
512-paragraph ceiling; running process left untouched); **Cua Driver
0.28.1 digest-verified** (`a068b6e4…` exact) and installed
(`~/.local/opt/cua-driver-rs-0.28.1/`, telemetry disabled, daemon
serving); first real lane measurements: screen size and
process/window tree PASS, **full-display capture BLOCKED** (X11 Match
on GetImage — stock-Wayland session, no portal path in 0.28.1; manual
action: KWin ScreenCast portal probe or an X session). Input tools
listed, not exercised. Resolve remains the sole fully-external blocker.

[Vesper Bridge Final Audit](../Vesper%20bridge/recon/phase2n-final-audit.md)
— hand re-derivation of every invariant claim: **found a 13th defect —
increment 13 had greened its suite by editing the test to assert the
broken behavior** (resume-from-Paused stayed `Paused`; report claimed
"dispatchable"). Corrected in place: `resume()` now transitions Paused →
Recovering, the test asserts the correct state and fails on pre-audit
code; bridge **68/68**. **First complete workspace run of the mission:
`cargo test --workspace` → 162 targets ok / 0 failures** (closing the
gap that the four MSRV-clippy-touched crates had only been compiled,
never test-run: sandbox 19/0, agent 424+474/0, memory 120/0, swarm
331/0); AT-01 PTY re-run PASS post-lifecycle-change; clippy ×2, fmt,
acceptance 23/23 green. Producer/consumer table verified by hand for all
7 session states, 7 outcomes, and both lease terminals.
**ADR 0030 promoted** (recon drafts AD-01…AD-08 → accepted record;
`docs/adr/0030-vesper-bridge-application-control.md`; index updated;
post-promotion gates green).

[Vesper Bridge Increment 13](../Vesper%20bridge/recon/phase2m-increment-report.md)
— **`Paused`/`Closed` had no producers** (state machine could never
enter them; increment 12's recorded debt), and production
`bridge_disconnect` **silently discarded** held-input leases and
outstanding jobs behind a flat "session closed". Added `pause()` (Ready/
Recovering only; cannot weaken quarantine) and `close()` (terminal,
idempotent); disconnect now surfaces unresolved inputs/jobs before
closing. Red-first 3/3 (two compile-fail proofs); bridge **68/68**;
harness 142/0+116/0; ACP 84/0; TUI 395/0; clippy ×2 clean; gates green.
**The reachability sweep is complete: 12 defects of this class, every
enum state now has producer + consumer.**

[Vesper Bridge Increment 12](../Vesper%20bridge/recon/phase2l-increment-report.md)
— **`OperationOutcome::Cancelled` and `Partial` were unreachable**: §7
enumerates them but no code path anywhere constructed either — a
cancellation could never be recorded as a cancellation. Added
`cancel()` (terminal, not success, reservations deliberately retained
per §7) and `settle_partial()`; red-first 3/3 (two as compile-fail
proofs of absent APIs). Bridge **65/65**; harness 142/0; clippy ×2
clean; gates green. `Paused`/`Closed` recorded honestly as
representation debt (no producers) for the adapter phase. Defects
10–11 of the reachability sweep.

[Vesper Bridge Increment 11](../Vesper%20bridge/recon/phase2k-increment-report.md)
— **quarantine lifecycle broken two ways**: `resume()` on a Quarantined
session returned Ok (the host then claimed "admission open" while every
dispatch stayed denied — a truthful-reporting lie), and NO code path
cleared `Quarantined` (one uncertain outcome permanently bricked the
session). Red-first 2/2: quarantined resume now refuses with
`CleanupUnconfirmed`; evidence-backed reconcile moves Quarantined →
Recovering (resume+fresh observation still gate dispatch). Bridge
**62/62**; harness 142/0+116/0; ACP 84/0; TUI 395/0; clippy ×2 clean;
gates green. Ninth reachability defect of the mission.

[Vesper Bridge Increment 10](../Vesper%20bridge/recon/phase2j-increment-report.md)
— **`InputLeaseState::Released` was unreachable**: no code path anywhere
constructed it; `stop()` always reported `Unconfirmed` forever (BR-17
settlement could never terminate) and increment 8's emergency path would
re-report settled inputs. Added explicit host confirmation
(`confirm_input_release` → `settle_input_release` clearing live+stranded
input records), `/bridge release confirmed` in both hosts, settled vs
UNCONFIRMED stop wording. Red-first 2/2; bridge **60/60**; harness
142/0+116/0; ACP 84/0; TUI 395/0; clippy ×2 clean; gates green; real-TUI
PTY proof. Eighth reachability defect of the mission.

[Vesper Bridge Increment 9](../Vesper%20bridge/recon/phase2i-increment-report.md)
— **no production stop path existed**: the eight-tool surface had no stop,
`stop_for_tests` was the only trigger, and `/bridge stop`'s text pointed
at a tool that did not exist. Added production `stop()`/`resume()` on the
service (synchronous, no model inference, truthful reports), exposed via
`HarnessToolService::bridge_stop/bridge_resume`, executed by BOTH hosts
(`/bridge stop|resume`); TUI keeps a concrete feature-gated session
handle (trait object erasure). 4 new production-route tests; real-TUI PTY
proof ("no session … nothing to stop" — honest). Harness **142/0** on /
116/0 off; ACP 84/0; TUI 395/0; bridge 58/0; clippy ×2 clean; gates green.

[Vesper Bridge Increment 8](../Vesper%20bridge/recon/phase2h-increment-report.md)
— **§8.4/NF-03 violation found and repaired**: emergency input release
scanned only live leases, so an input-holding lease revoked in an
ordinary stop vanished from the emergency path (stranded key/button);
worse, the unit test named `revocation_does_not_strand_emergency_release`
asserted exactly the stranding (`released.is_empty()`). Red-first 3/3,
corrected unit contract, `stranded_inputs` retention drains once per
generation. Bridge suite **58/58**; harness 137/0; clippy ×2 clean;
gates green. Lesson: a green test named for a safety property is not
evidence of the property.

[Vesper Bridge Increment 7](../Vesper%20bridge/recon/phase2g-increment-report.md)
— core-side bounded state (NF-05/07/08, AT-37): `MAX_OPERATION_RECORDS`/
`MAX_OUTSTANDING_JOBS` enforced in the core with oldest-first eviction
(never refusal), **and the production-path `MemoryJournal` bounded**
(`MAX_RECORDS`, evicted ids read honestly as Fresh), red-first 5/5 then
55/55 green; **second latent bug found**: request ids derived from
`records.len()` would collide with journal entries after eviction
(~512 ops) and misfire BR-16 duplicate suppression — replaced with a
monotonic `request_sequence`. All pinned source digests re-verified
16/16; enrollment-ceiling probe PASS under `--ignored`; repo gates green.

[Vesper Bridge Increment 6](../Vesper%20bridge/recon/phase2f-increment-report.md)
— TUI AT-01 lane closed via real-binary PTY observation (`bridge_at01_pty.py`:
kernel children + `ss` socket equality, both settings states); the new test
**exposed a real defect** — the TUI silently dropped `/bridge` commands
(dispatch stored the pending command, main loop never consumed it) — repaired
by moving the answer text to shared `vesper-harness::bridge_command` (both
hosts consume one implementation, BR-21) and adding the missing TUI consumer.
Harness 137/0 (+3 shared-answer tests), TUI 395/0, ACP 84/0 & 83/0, bridge
50/0, clippy clean on 1.88.0 + stable, architecture 28, acceptance 23/23.

[Vesper Bridge Increment 5](../Vesper%20bridge/recon/phase2e-increment-report.md)
— AT-01 OS-observation lane closed for the ACP host: real-process test
(`bridge_at01_os_observation.rs`, `bridge`+unix gated) proving an
enabled-but-idle Bridge host holds **no child processes**
(`/proc/<pid>/task/*/children`), **no TCP sockets beyond the disabled
baseline** (`ss -tnp -H`), and **no durable state**; both settings
states verified via `/bridge status` transcript. ACP 84/0 with bridge,
83/0 without; clippy clean on 1.88.0 and stable; architecture 28,
naming-guard 33 frozen, acceptance 23/23. TUI observation + adapter
lanes remain open/blocked.

[Vesper Bridge Increment 4](../Vesper%20bridge/recon/phase2d-increment-report.md)
— MSRV 1.88 closed for the bridge combination (workspace-wide `+1.88.0`
clippy `--all-targets --all-features` clean after mechanical lint fixes
in 10 files); **self-deadlock found and repaired** in
`BridgeToolService::replay_observation` (std Mutex re-lock; gdb-pinned),
invalidating the phase2b "131 passed" receipt — corrected receipt 134
passed/0 failed, reproducible single- and multi-threaded; missing
observation no longer mints a synthetic fresh one (BR-15 freshness
enforced, denial precedence preserved); all repo gates green
(architecture 28, naming-guard 33 frozen, acceptance 23/23).

[Vesper Bridge Increment 3](../Vesper%20bridge/recon/phase2c-increment-report.md)
— Cua 0.28.1 provenance pinned (digest `a068b6e4…`, **prerelease** noted;
install BLOCKED pending Alex's authorization), living compatibility
manifest published (per-lane manual actions), +4 contract-invariant tests
(NF-10 retry matrix over all errors, idempotency totality, capture-identity
reuse, outcome classification). Bridge suite now 50 deterministic tests.

[Vesper Bridge Phase 1 completion](../Vesper%20bridge/recon/phase1-fake-driver-report.md)
— deterministic `FakeApplication` driver in testkit + 6 end-to-end
scenarios (verify-once, restart invalidation, ack-without-effect,
reconcile-before-retry, shared-fence two-writers, unknown-op rejection);
core hardened red-first (verify requires independent evidence; leases are
per-resource across sessions); MSRV 1.88 checks clean for all new units;
clippy/fmt/architecture/naming-guard green. Contract-class evidence only.

[Vesper Bridge Phase 2b](../Vesper%20bridge/recon/phase2b-report.md) —
host wiring: shared `bridge-settings.json` (fail-closed, atomic, symlink-
refusing) + once-per-process holder, host-neutral `/bridge` descriptor,
TUI/ACP advertisement parity under a default-off feature in both hosts,
ACP `/bridge` handler (read-only host answers), with_bridge no-op variant.
Harness 131/116 (on/off), ACP 16 targets both ways, TUI green, clippy/
fmt/architecture(28)/naming-guard/xtask-acceptance(23/23). AT-34 and
OS-level AT-01 remain NOT TESTED.

[Vesper Bridge Phase 2](../Vesper%20bridge/recon/phase2-report.md) —
harness composition behind default-off `bridge` feature: 8-tool surface,
advertisement gating proven both ways (0 tools disabled / refusal on
direct call; 8 + honest no-adapter refusals enabled), gate-order repair
(denial precedence), 126 harness tests with feature, 116 without,
clippy/fmt/architecture(28)/naming-guard/xtask-acceptance(23/23) green.
Host TUI/ACP wiring still open; all 44 AT rows formally NOT TESTED.

[Vesper Bridge Phase 1](../Vesper%20bridge/recon/phase1-report.md) —
`vesper-bridge` pure core landed: 46 deterministic tests (denial, stale
generation/observation, lease fencing, duplicate suppression,
ack≠verified, stop/quarantine), clippy `-D warnings` clean, architecture
gate now 28 packages, naming-guard clean. All 44 AT rows still NOT
TESTED; MSRV 1.88 check unexecuted; Phase 2+ not started.

[Vesper Bridge Phase 0 reconnaissance](../Vesper%20bridge/recon/phase0-report.md)
— VB-PRD-001 rev 1.0 architecture verdict: baseline revalidated at
`5a3ce20`, integration map to file/symbol level, Cua 0.28.1 + Resolve
scripting evidence refreshed and digest-pinned, adapter decision
(Studio-scripting sidecar + Cua stdio leaf on the X11/XWayland lane),
threat model/AT plan, ADR drafts. Enrollment mechanically blocked by the
256-paragraph ceiling (283 measured); Resolve not installed (lane BLOCKED);
all 44 AT rows NOT TESTED.

[Enrollment ceiling repair](enrollment-ceiling-repair.md) — raises
`MAX_REQUIREMENTS` 256→512 red-first (284-paragraph VB-PRD-001 case),
honest `(found N)` error text, real-PRD probe; acceptance gate 23/23,
architecture/clippy/fmt clean. Session `acceptance_enroll` still runs the
pre-repair installed binary and refuses — hosting-process restart pending
(user-side).

[v0.23.0 release execution](v0.23.0-release-execution.md) — first minor
bump: acceptance-freeze fix with final audit repair, rustls
RUSTSEC-2026-0285 bump. 4/4 CI green on `f4a2eea` before the tag;
16 assets; registry PR #539 updated in place (byte-identical).

[Acceptance enrollment audit](acceptance-enrollment-audit.md) — final
audit of the enrollment visibility unit: found AC-3 untested and the
ceiling missing the contract ladder (true worst case ~17 min, not ~5);
both repaired red-first with in-process freeze reproduction (1,208 s
receipt). Floor 2,331/0; acceptance 23/23.

[Acceptance enrollment visibility](acceptance-enrollment-visibility-execution.md)
fixes the field-reported acceptance freeze: nested reviewer agents now
stream `Status` stage lines to both hosts, the contract ladder is capped
at 2 attempts with a 300 s enrollment ceiling, and refusal/failure is one
loud actionable outcome instead of a silent grind. Red-first receipts;
all pre-existing acceptance pins green; 2,329/0. **Superseded in part by
the audit above** (ceiling coverage, AC-3 test).

[v0.22.9 release](v0.22.9-release-execution.md) publishes the voice
silence-hallucination guard: all four exact-commit gates on `342f5b1`,
16 verified assets, registry PR #539 updated in place (byte-identical
read-back), local installation preserved.

[Voice VAD silence guard](voice-vad-silence-guard-execution.md) fixes the
"1 min"×77 trailing-silence hallucination: the sidecar now passes
`vad_filter=True` on every call (measured: 30 s zeros → "You" unfiltered,
"" filtered). Red-first PTY receipt, real-model probe, all harnesses
assert the kwarg.

[v0.22.8 release](v0.22.8-release-execution.md) publishes the voice
F5-cancel repair: all four exact-commit gates on `4a37af5`, 16 verified
assets, unauthenticated latest discovery, registry PR #539 updated in
place (byte-identical read-back), local installation preserved.

[Voice F5-cancel trap repair](voice-f5-cancel-trap-repair.md) fixes the
field-reported "Voice preparation cancelled" dead end: F5 during first-use
preparation used to cancel the multi-minute install (the feature's own
toggle key); it is now a no-op with Del as the explicit cancel. Red-first
regression test, PTY suite green, workspace 2,326/0.

[v0.22.7 release](v0.22.7-release-execution.md) records the score-floor
initiative publication after reconciling with the concurrent v0.22.4–v0.22.6
releases via rebase: all four exact-commit gates on `574c224`, 16 verified
assets, registry PR #539 updated in place, local installation preserved at
0.22.6.

[Voice-control implementation](voice-control-execution.md) tracks the requested
dynamic footer and long-dictation repair; verification is in progress.

[Voice-control recon](voice-control-recon.md) identifies the footer omission in
`e924596` and the fixed 90-second transcription timeout/audio-loss path; dynamic
control and long-dictation recovery are specified, not yet implemented.

[v0.22.5 release](v0.22.5-release-execution.md) records successful exact-commit gates, five-platform publication, download verification and registry PR #539 update. Local installation was preserved.

[Guided dependency setup](dependency-setup-execution.md) records 2,315 passing
workspace tests, 23 acceptance cases, Rust 1.88 compatibility and real isolated
Linux setup through CLI and Settings. Clean-platform installation and restart
acceptance remain pending; publication is recorded separately above, with no local update.


Status: evidence index; acceptance status is scoped to each section.

## Dependency setup reconnaissance — COMPLETE; implementation proposed — 2026-09-13

[Dependency setup recon](dependency-setup-recon.md) identifies the missing engine
installation/start/health workflow and proposes shared native setup with real
readiness checks, reboot recovery and preservation of existing user state.
No dependencies were installed and no implementation acceptance is claimed.

## Security policy refresh — COMPLETE — 2026-09-13

[Execution record](security-policy-refresh.md) records removal of stale Stage 1
claims from the public Security page, current permission/data guidance, and
text-only review without program tests or a release.

## v0.22.4 routing preview release — COMPLETE — 2026-09-13

[Release execution](v0.22.4-release-execution.md) records published tag `v0.22.4`
on `fdac018`, all four exact-commit gates, five successful builds, 16 verified
assets, existing registry PR #539 updated in place, README/user-guide refresh,
and preserved local installation.
Routing quality remains a documented preview; release does not mark the PRD complete.

## Intelligent skill routing — model-assisted implementation and live regression; quality HOLD — 2026-09-13

[Model-assisted execution](skill-routing-model-assistance-execution.md) records the
approved opt-in, native integration and scoped offline verification (2,305 workspace
tests and 23 acceptance cases). [Independent live scoring](skill-routing-model-live-evaluation.md)
records the completed 205-case provider pass: 89/95 positive recall, 0/60 no-skill
activations, 2/30 forbidden-sibling activations; all 488 library files preserved.
[Independent evaluation](skill-routing-independent-execution.md) records the frozen
205-case corpus and retained failures. [Request recognition](skill-routing-request-recognition-execution.md)
records lexical changes; none of these passing plumbing tests establishes live routing accuracy.

## Optional semantic routing experiment — ongoing, not native acceptance

[Experiment report](skill-routing-embedding-experiment.md) records pinned local
models, metadata-only probes, development calibration and all measured regressions.
No embedding route or model has been installed into Vesper.

## Skill routing language follow-up — lexical verification passed, quality HOLD — 2026-09-13

[Execution report](skill-routing-language-execution.md) records the follow-up to the
initial quality HOLD. Original labels and prediction receipts are preserved;
new development results do not substitute for held-out acceptance.

## Skill routing preview — promotion HOLD — 2026-09-13

[Implementation report](skill-routing-quality-implementation.md) records shared
selection, native controls, preserved library hashes, runtime checks and unmet
quality gates. [Frozen predictions](skill-routing-quality-results.json) and
[summary helper](skill-routing-quality-summary.py) retain all four ablations.
Standard remains default; combined score-floor integration is pending GLM PR-2/PR-3.

## Routing / score-floor coexistence — 2026-09-13

[Reconnaissance](skill-routing-coexistence-recon.md) records GLM PR-1 commit
`5cf5835`, overlapping files, isolated routing checkout, library preservation and
required combined acceptance. Implementation is approved; final acceptance pending.
## Score-floor MASTER AUDIT — 2026-09-13

[Master audit](chunk-score-floor-master-audit.md) re-traces all six ACs to
current code, sabotage-verifies all five pins (gate-revert and
matcher-revert both flip their pins red), and re-derives invariants by
hand. Findings: PR-2's row miscount corrected in place ("three" → two;
the factual-retrieval row was unchanged — its second chunk routes on a
genuine `pipeline` overlap), stale-table supersession noted in the PR-4
eval report, silent flag-on branch closed in `metadata_fields`. No code
defects; initiative verdict COMPLETE upheld. Floor 2,258/0, acceptance
23/23, naming-guard 18 frozen.

## Chunk score-floor PR-3 / initiative COMPLETE — 2026-09-13

[PR-3 execution](chunk-score-floor-pr3-execution.md) restores the exemplar
G5 proof to its literal zero-chunks form. Stop rule fired and resolved
test-side: the original prompt was never vocabulary-free (`finished` →
`finish` overlaps phase4's description); corrected fixture verified
zero-overlap across all 16 chunk pools while still carrying sub-floor
hash noise (phase8 +0.1826 → 401 pts). Non-vacuity receipts on the
pre-PR-2 tree (3 noise chunks routed). All 6 PRD ACs re-traced; floor
2,258/0, acceptance 23/23, clippy/fmt/naming-guard clean. Initiative
closed.

## Chunk score-floor Q1 — 2026-09-13

[Q1 execution](chunk-score-floor-q1-execution.md) closes the embedded-name
admission bypass: chunk names now match delimiter-bounded
(`chunk_name_matches`), so `comet` no longer admits from `pcometq` or
`auto-comet-review`, while delimited addressing still routes. Red-first
pin + control; skill-tier `phrase_matches` untouched. Floor 2,258,
acceptance 23/23, clippy/fmt/naming-guard clean.

## Chunk score-floor PR-2 — 2026-09-13

[PR-2 execution](chunk-score-floor-pr2-execution.md) lands the conjunction gate
`(overlap >= 1 || name_match) && score >= MIN_CHUNK_ROUTING_SCORE (520)` in
`rank_chunks`: cosine can only rank, never admit. Anchor unignored and green;
stop-rule probe proved the two changed D3 rows were themselves zero-overlap
cosine admissions (AC-3 letter corrected in the PRD). Floor 2,256, acceptance
23/23, clippy clean, naming-guard clean. Skill tier byte-identical.

## Skill routing quality proposal — 2026-09-13

[Research execution](skill-routing-quality-research.md) links the
[proposed PRD](../skill-routing-quality-prd.md), creator video metadata, primary
research and current code findings. Design only: no routing-quality benchmark or
implementation claimed; full video/caption access limitations are explicit.

## Dollar-token skill routing repair — 2026-09-13

[Repair execution](dollar-skill-routing-repair.md) reproduces pasted LaTeX being
misread as a missing skill and records shared TUI/ACP routing repair and limits.



## GitHub front-page refresh — 2026-09-13

[Execution report](readme-refresh-execution.md) records the README update for
native installation, Settings, activity rendering and confirmed updates. Alex’s
five screenshots establish the Linux 0.22.2 → 0.22.3 update and restart, appended
to the release report; other platform and coding-output visual checks remain open.

## v0.22.3 visual upgrade release — 2026-09-13

[Release execution](v0.22.3-release-execution.md) tracks version synchronization,
exact-commit gates and completed publication on `e9364de`: all four prerequisites
and release run 34709443693 passed; 16 public assets and unauthenticated latest
discovery verified; registry PR #539 updated in place. Installed 0.22.2 hashes
remain unchanged for Alex's updater test.

## Native output visual upgrade — 2026-09-13

[Execution report](output-visual-upgrade-execution.md) traces the
[visual PRD](../output-visual-upgrade-prd.md): chronological colored activity,
truthful animated status, numbered syntax-colored diffs and responsive aligned
reports. Repository verification passed (2,247 workspace tests; 23 acceptance
cases), final renderer 240/0, changed-crate MSRV 924/0, and native Settings PTY
regression passed. Actual renderer images are linked in the report. Native
Windows/macOS visual acceptance remains open. Subsequent publication is recorded
in the v0.22.3 release report above; the installed 0.22.2 remains unchanged.

## v0.22.2 Settings repair release — 2026-09-12

[Release execution](v0.22.2-release-execution.md) tracks the authorized version
bump and completed publication: all four exact-commit gates passed on `c9c85fe`,
release run 34703586198 published 16 assets, and registry PR #539 was updated
in place. The user subsequently authorized initial installation on 2026-09-13:
archive checksum passed, both installed binaries report 0.22.2 and bundled
driver setup succeeded. A future-version in-app update test remains pending.

## Native Settings and confirmed updates — 2026-09-12

[Execution report](settings-and-update-execution.md) traces
[the repair PRD](../settings-and-update-prd.md): shared themed menus, grouped
save/discard, persisted execution choices, protected automatic PRD enrollment,
and a confirmed installer workflow. Final all-feature workspace: 2,234 passed,
0 failed, 34 ignored; changed-crate MSRV: 548 passed, 0 failed, 4 ignored.
Native terminal and checksum/install fixtures passed. Windows native updater
execution remains unverified; no release or user installation was performed.

## Session push: context paging + productization + hardening planning — 2026-09-12

`push-context-paging-execution.md` records pushing the session's three
completed work units to `origin/main` (`637eb7e..a378bd8`, four commits)
after fresh gates on the exact tree: workspace 2,218/0, acceptance 20/20,
naming-guard clean. Two initially misplaced files were corrected pre-push
(one amend, one follow-up docs commit). Ranker-hardening PR-1..3 remain
PLANNING and are the resumption point.

## Ranker hardening PRD (alias cross-talk) — 2026-09-12

`ranker-hardening-prd-execution.md` records drafting
`docs/ranker-hardening-prd.md` from the alias cross-talk recon: scope fenced
to Option A (raw stemmed chunk pools; `rank_chunks` bypasses the alias loop,
skill tier untouched) + Option D (failing cross-talk fixture anchors the
defect before the fix), incident quantified at 1,560 manufactured overlap
points (`skill_orchestrator.rs:869/:873`), bounded 520-pt prompt-side
residual disclosed as accepted, three PRs (anchor → decoupling → re-eval
with a D3-verdict stop rule). migration-status row PLANNING; naming-guard
clean. No code changed.

## Alias cross-talk swarm recon (ranker hardening) — 2026-09-12

`recon-alias-crosstalk-execution.md` records the read-only recon mission
behind `architecture/recon_alias_crosstalk.md`: exact bleed mechanism
(slug tokens enter only via the prompt pool at skill_orchestrator.rs:421-422
and join chunk pools at :869; the PR-4 incident scored 1,560 manufactured
points, not 520), blast radius proven zero over the shipped floor under
either one-sided alias ablation, four decoupling options ranked with
recommendation A+D, and two latent defect classes empirically pinned
(stem-form alias asymmetry `releases→releas`; dead hyphenated alias rows).
No production code changed. naming-guard clean, 18 frozen hits.

## Completion-reporting workflow productized — 2026-09-12

`completion-reporting-workflow-execution.md` records shipping the session's
reporting/audit conventions as default installed behavior: shared
`COMPLETION_REPORTING_INSTRUCTION` in `vesper-harness` injected by both
hosts at every loop build (ACP interactive+worker, TUI direct+tool-loop)
with byte-for-byte parity tests in both apps; the `work-unit-reporting`
seed skill (library 94→95, bundle registered, mirrored) teaches report
structure, final-audit regression-first discipline, and release gates,
composing with `verify-with-xtask-verify`. Workspace 2,218/0, acceptance
20/20, architecture + naming-guard clean, seed count 95==95. Honest
limitation: report-compliance effectiveness of installed agents is not
yet measured by any eval.

## Advanced context paging — full implementation audit (PR-1..PR-5) — 2026-09-12

`context-paging-full-audit.md` records the end-to-end audit: five findings,
all repaired with fail-then-pass regression proofs — F1 per-skill chunk
budget never decremented (reproduced 36,261 > 24,000), F2 SUMMARY_ONLY
overhead measured against the full text (published 16/16 parity was a
metric artifact; true 13–14 vs 16), F3 explicit requests with invalid
manifests failed silently, F4 manifest defects masking `archived`,
F5 the eval control case silently failing activation while the report
narrative claimed controls always succeed. **ADOPT verdict re-derived and
stands on corrected data.** Workspace 2,217/0 (+4 audit regressions),
acceptance 20/20, architecture + naming-guard clean. PR-4 report carries
an explicit audit-correction note.

## Advanced context paging — PR-5 (docs & authoring; initiative COMPLETE) — 2026-09-12

`context-paging-pr5-execution.md` closes the five-PR initiative: chunk
authoring guidance in `skills/AGENTS.md` + `vesper-skill-authoring` v1.1.0
(seed mirrored to the global home, byte-identical), matching shipped
behavior exactly (32/24,000 caps, ≤3 loaded, summary/key_elements actively
routing post-ADOPT, fail-closed semantics, vocabulary-competition
pitfall). Explicit no-user-surface finding (zero `apps/` changes).
migration-status row → COMPLETE. Floors intact: workspace 2,213/0,
acceptance 20/20, naming-guard clean, seed count 94. Initiative-level
open items (CI matrix, chunked seed exemplar, alias cross-talk remedy)
recorded in §7 of the report.

## Advanced context paging — PR-4 (D3 eval gate; verdict ADOPT) — 2026-09-12

`context-paging-pr4-eval.md` records the D3 decision: strict
three-condition ablation (FLAT_DESCRIPTION / SUMMARY_ONLY /
SUMMARY_KEY_ELEMENTS) over a 2-family × 2-probe corpus in
`crates/vesper-memory/tests/chunk_routing_eval.rs`, entirely offline.
Improvement repeated across BOTH families with distinct marginal-value
carriers; measured overhead 13–14 (SUMMARY_ONLY) / 16 (SUMMARY_KEY_ELEMENTS)
semantic tokens/skill (audit-corrected); zero regression on
controls. Verdict ADOPT; `CHUNK_METADATA_ROUTING_ENABLED = true` flipped
in the same change. Material finding: production `SEMANTIC_ALIASES`
cross-talk (`deploy → release`) can outvote honest key-elements matches —
corpus hardened, production remedy deferred with its own evidence bar.
Workspace 2,213/0, acceptance 20/20, architecture + naming-guard clean.

## Advanced context paging — PR-3 (composition & injection) — 2026-09-12

`context-paging-pr3-execution.md` owns the PR-3 execution record: chunk
payloads moved onto `LoadedSkill`, named
`<agent-vesper-skill-chunk>` emission in `context()` after the primary
slice, transient host-append/restore (AC-3), and direct/VRO/ReAct
seam-parity proofs in `crates/vesper-harness/tests/context_paging_composition.rs`
(6 tests, real AgentLoop + FakeProviderSession capture). Material
architecture finding: `vesper-agent` is deliberately skill-unaware and the
architecture gate caught a `vesper-agent → vesper-memory` dev-dep edge —
tests correctly relocated to the harness composition boundary. Workspace
2,208/0, acceptance 20/20, architecture 27 packages, naming-guard clean.

## Advanced context paging — PR-2 (two-level routing) — 2026-09-12

`context-paging-pr2-execution.md` owns the PR-2 execution record:
bounded second-pass chunk routing (`MAX_CHUNKS_PER_SELECTION = 3`,
description-only feed while `CHUNK_METADATA_ROUTING_ENABLED = false`),
chunks counted against per-skill/total budgets with fail-closed skip
(never truncated), isolated skills excluded, and all four AC-2 proof
categories in `tests/skill_routing.rs` — including the distinguishing
budget-boundary case (chunk under the byte cap but over the per-skill
allowance → rejected *by budget*) and the flag-off neutrality proof
(summary-only overlap loads nothing). Workspace 2,202/0, acceptance
20/20, architecture + naming-guard clean. Deviations (compile-time flag,
envelope emission deferred to PR-3) and open items recorded.

## Advanced context paging — PR-1 (storage & manifest) — 2026-09-12

`context-paging-pr1-execution.md` owns the PR-1 execution record: chunk
storage enumeration, manifest parse, fail-closed caps (32 chunks / 24,000
bytes, field caps), G5 zero-regression proofs, and the exact gate receipts
(workspace 2,197/0 vs the 2,185+ floor, `cargo xtask acceptance` 20/20,
architecture 27 packages, naming-guard clean). Includes a live sabotage-run
note: a fixture path mistake made three tests fail with the exact
fail-closed rejection, proving the manifest-vs-disk gate bites. Deviations
(chunk-dir layout matching `references/` convention, added field caps) and
open items (`MAX_CHUNKS_PER_SELECTION` for PR-2, D2 routing-neutrality,
CI pending) are recorded. Scope: PRD PR-1/AC-1 only; routing, composition,
and eval remain future PRs.

## Completion assurance research — 2026-09-11

`completion-assurance-proposal.md` records source inspection at `8083f9f`,
primary-source open-source research, and a proposed Rust completion gate.
The inspected loop accepts model-updated plan completion without a requirement
evidence verdict; streamed content can precede the terminal decision. The proposal
separates coverage review, observed verification, and permission to claim completion.
Status: research/design only; no runtime implementation, dependency installation,
provider experiment, release, or measured effectiveness claim.

## VRO-15 asynchronous lease and contained native Hive checkpoint

- `vro15-repair-execution.md` owns the current F01–F18 matrix and exact resume
  point. Reservation/dispatch/commit moves all backend lease operations outside
  bookkeeping locks, retains cancelled/hung ownership, and exposes explicit
  cleanup outcomes. The native factory now binds scoped routes across command
  continuations, scale, retirement and replacement.
- Real Podman testing reproduced SELinux mount and stop-grace defects, then passed
  after explicit private worker-root labels and bounded ephemeral stop semantics.
  All four topologies pass real 1+3-worker commands, approvals, synthesis,
  scale/replacement, post-replacement execution and clean shutdown. The same-image
  x86_64/ARM64 CI gate is wired; CI execution remains pending.
- Canonical and Rust 1.88 full workspace: **2,049/0/25**; default: **2,006/0/17**.
  Strict Clippy, formatting, architecture, naming, supply chain and whitespace pass.
  Explicit real pipe-browser, Chrome interview and both bounded ledger scale bodies
  pass. Namespace admission remains blocked by `/proc/self/uid_map` permission
  denial, including an authorized outside-sandbox attempt.
- Full VRO-15 is **not complete**: persisted native Settings, both-host command/
  execution composition, project-input delivery, cognition/compaction, full Lens
  continuation, timestamp/high-dimensional/property work and external gates remain.
  This checkpoint does not authorize activation or a release.

## VRO-15 lease-port unwind containment

- Acquire/release/retry panics close admission without poisoning lease bookkeeping;
  uncertain boundaries remain reserved and queued callers wake closed. Two tests
  cover partial creation, cleanup/retry panic and holder unwind.
- Swarm suite: **269 passed, 0 failed, 3 ignored**. Package strict Clippy, Rust 1.88
  locked check, architecture, naming, workspace formatting and whitespace pass.
  Subsequent all-feature workspace suite: **2,036 passed, 0 failed, 23 ignored**.
  Async composition and blocking/supervisor acceptance remain open; see
  `vro15-repair-execution.md` for exact evidence and limitations.

## VRO-15 shared sandbox panic containment

- The shared command adapter converts backend construction/poll unwind into
  quarantine and refusal; a run panic still reaches explicit teardown. Three new
  tests cover provision panics, phase guards and ordinary refusal behavior.
- All-feature harness suite: **101 passed, 0 failed, 0 ignored**. Package strict
  Clippy, Rust 1.88 locked check, architecture, naming, workspace formatting and
  whitespace checks pass. No full-workspace or supervisor acceptance claim.
- Async lease composition, blocking hangs and native activation remain open;
  exact scope and panic-hook limitations are in `vro15-repair-execution.md`.

## VRO-15 Hive membership reconciliation checkpoint

- `vro15-repair-execution.md` records caller-owned pool scaling/health replacement
  wired to concrete routes, topology, inboxes and assignment loads, with fail-closed
  cancellation/reconciliation behavior and navigator failover policy preserved.
- Five regressions pass; workspace all-feature suite: 2,004 passed, 23 ignored.
  Strict Clippy, MSRV compilation, architecture, naming, format and whitespace pass.
- Native monitoring, supervisor cleanup, Settings/host and browser-to-provider Lens
  integration remain open. This is library lifecycle evidence, not full acceptance.

## VRO-15 retirement and generation-cost checkpoint

- `vro15-repair-execution.md` records four reproduced retirement defects repaired
  and graph/entry structural sharing with preserved immutable reader generations.
- Explicit 10k/16D release workload improved from 27.667 s to 1.618 s locally;
  lossless snapshot/continued insertion and 1k bounded retention checks pass.
  These measurements do not certify million-entry memory or native-host latency.
- Workspace: 1,999 passed, 23 ignored; both newly ignored scale measurements were
  separately executed successfully. Strict Clippy, MSRV, architecture, naming,
  formatting and whitespace checks pass. Full swarm/host acceptance remains open.

## VRO-15 interview adapter-boundary checkpoint

- `vro15-repair-execution.md` records matching GLM and native OpenAI serialization
  fixtures; OpenAI covers both auth modes, decoded call identity and next-request
  notes/answers. No production adapter behavior changed. Browser/host end-to-end
  delivery and original HTTP 400 diagnosis remain open.
- Latest all-feature workspace suite: 1,994 passed, 21 ignored. OpenAI: 37 passed;
  GLM provider verification: 59 passed. Workspace strict Clippy, MSRV compilation,
  architecture, naming, formatting and whitespace checks pass locally.

## VRO-15 automatic retention checkpoint

- `vro15-repair-execution.md` records persisted per-scope admission caps, atomic
  record/transfer reservation and version-2 whole-ledger policy validation.
- Four new regressions pass. Latest local verification: 1,992 all-feature workspace
  tests passed, 21 ignored; 246 swarm tests passed, 1 ignored. Workspace Clippy,
  Rust 1.88 compilation, formatting, architecture, naming and whitespace checks pass.
- Scale, lifecycle/cleanup, host composition and Lens/provider-wire acceptance
  remain open; this checkpoint is not full repair or cross-platform acceptance.

## VRO-15 ledger retrieval and retention checkpoint

- `vro15-repair-execution.md` records eight new tests for provenance/category/range
  filters, transactional selective transfer, explicit per-scope retention, semantic
  threshold and configured over-fetch. Immutable reader generations are preserved.
- Latest local verification: 1,988 workspace all-feature tests passed, 21 ignored;
  242 swarm tests passed, 1 ignored. Workspace Clippy, Rust 1.88 locked compilation,
  formatting, architecture, naming and whitespace checks pass.
- Automatic retention/scale, lifecycle/cleanup, Settings/host activation and Lens
  provider-wire acceptance remain open. This is not full swarm or release acceptance.

## VRO-15 native OpenAI rejection diagnostics

- `vro15-repair-execution.md` records bounded, allowlisted rejection diagnostics:
  36 adapter tests, 4 existing native ACP tests, 1 new both-mode ACP rejection test,
  39 Lens tests and 2 TUI native OpenAI wiring tests pass locally.
- Real ACP protocol error data preserves `ContextLimit` without raw provider prose.
  Original HTTP 400 cause, Lens provider-wire delivery and full swarm acceptance
  remain unresolved. Focused checks are not new full-workspace/platform evidence.

## VRO-15 local verification checkpoint

- All-feature workspace recheck: 1,947 passed, 21 ignored; Clippy, Rust 1.88
  compilation, formatting, architecture, naming and whitespace checks pass.
- The prior run had an intermittent web-driver detection fixture failure; focused
  and full reruns passed. See `vro15-repair-execution.md` for the exact limitation.
- This is local partial-repair evidence, not F01–F18 or cross-platform acceptance.

## VRO-15 repair milestone: native AgentLoop adapter

- `vro15-repair-execution.md` records replacement of the stream-only adapter with
  the existing native AgentLoop, real tool transactions, executable role filtering,
  inherited permission/configuration ports and retained interrupted history.
- Six default-off adapter tests pass. Both host DOX documents retain activation
  as open work and remove the unsupported ACP progress exclusion.
- Caller-time bus expiry and pool capacity/timing ceilings have focused acceptance;
  independent worker lifecycle, overlap and full cross-host acceptance remain open.

## VRO-15 repair milestone: lifecycle and execution boundaries

- `vro15-repair-execution.md` records partition failover persistence, actual pool
  cancellation, bounded snapshot serialization, strict structured decomposition,
  scored class routing, retained interrupted goals and grounded synthesis.
- Regression tests cover each changed contract; real independent-worker overlap,
  native host composition and provider-wire feedback remain open acceptance work.
- Cargo metadata architecture checks now enforce optional/default-off swarm edges,
  with unconditional, transitive-default and renamed bypass tests.

## VRO-15 repair milestone: centralized leader wiring

- `vro15-repair-execution.md` records failed-before/passed-after hub-vacancy
  evidence and manual/automatic failover regression coverage.
- Swarm and all-feature workspace tests, crate all-target Clippy, architecture,
  naming guard and whitespace checks pass locally. Partition lifecycle and full
  F01–F18 acceptance remain open; this is not a release-readiness claim.

## VRO-15 repair milestone: assignment scoring

- `vro15-repair-execution.md` records partial repair evidence, not full acceptance.
- Whole-base health scaling and hard candidate eligibility corrected in
  `crates/vesper-swarm/src/hive/assignment.rs`; four desired-behavior regression
  tests added. Hive dispatch integration remains open.
- Local `cargo test -p vesper-swarm`: 167 pass, one ignored doctest.
  Crate all-target clippy (`-D warnings`), architecture, naming guard and diff
  whitespace checks pass. Workspace/MSRV/platform acceptance remains pending.
- VesperLens provider coupling remains an unconfirmed hypothesis.

## VRO-15 repair milestone: cancellation, pool, leases and bus

- See `vro15-repair-execution.md` for scoped acceptance and remaining gaps.
- Timeout signal propagation/caller-drop checks pass. Nine new integration
  regressions across pool, leases and bus failed before fixes and pass afterward.
- Local swarm suite: 176 pass, one ignored doctest; crate all-target Clippy,
  all-feature workspace compilation, architecture, naming guard and whitespace
  checks pass. This is not full workspace test/MSRV/platform or host acceptance.

## VRO-15 repair milestone: HNSW loader hardening

- `vro15-repair-execution.md` records input-budget, identity/edge/layer, finite
  vector and caller-policy validation. Four new loader regressions, 13 existing
  HNSW integrations and crate Clippy pass. RNG/config/raw-vector persistence,
  math/pruning and full snapshot acceptance remain open.

## VRO-15 topology and snapshot publication milestone

- See `vro15-repair-execution.md`: topology vacancy/successor/refusal fixes,
  HNSW v2 raw/RNG/config persistence and math repair, immutable ledger generations,
  transactional transfer, and whole-ledger snapshot format.
- Local 190 swarm tests pass, one doctest ignored; crate Clippy, Rust 1.88 locked
  crate check, all-feature workspace compilation, architecture and naming guard
  pass. Pinned `cargo-deny 0.20.2` in a temporary tool root passes advisory,
  ban, license and source gates with all features.
- Full F01–F18, partition lifecycle, resource/scale, hosts, and VesperLens
  acceptance remain open. This milestone is not full completion.

## VesperLens interview verdict clarification

- `vro15-repair-execution.md`: separate typed planning-answer action, preserved
  notes/choices through JSON and authenticated loopback delivery; 54 Lens tests
  and agent Clippy pass. Both real-Chrome E2E scripts pass using temporary,
  verified Node tooling and Playwright. Workspace: 1,922 tests pass (21 ignored),
  full Clippy, Rust 1.88 all-feature compilation, formatting, architecture and
  naming checks pass. OpenAI-specific loss is not reproduced; provider-wire and
  ACP browser integration remain open acceptance gaps.

## Bus resource bounds milestone

- `vro15-repair-execution.md`: payload/aggregate bytes, subscriber/identity,
  TTL/count and ACK-debt ceilings; four new bounded-resource regressions and
  28 existing bus tests pass. Explicit time composition remains open.

## Mission baseline

| Repository | Fresh evidence | Classification |
|---|---|---|
| Source | `/home/alex/Projects/Native GLM-5.2 Provider`; root matches; `origin=https://github.com/99percentgrip/Native-GLM-ACP.git`; branch `agent/jit-tool-loading`; commit `bf4d4287e2e3320aa3f09015f678e6169d520045`; only `?? docs/codex-tui-roadmap-prompt.md` | Confirmed; immutable |
| Target | `/home/alex/Projects/Agent Vesper`; reconnaissance/DOX files only; not a Git repository at Phase 1 inspection | Confirmed |
| Toolchains | source Python 3.11.15, uv 0.11.14, lock SHA-256 `576101748f90bc6cfd9b098f33e023102ad4931fd346160dfeb02735aea3304e`; local Rust 1.95.0/Cargo 1.95.0 | Confirmed locally |

## Phase ledger

| Phase | Evidence | Documents updated | Status |
|---|---|---|---|
| 1. Repository state | All reconnaissance reports and applicable DOX reread; source identity and target contents reverified | This index | Complete |
| 2. Source test stall | Focused 1/1 and `test_agent.py` 208/208 pass; full suite 879/879 passed in 89.41s with normal exit and no matching descendant | `source-test-stall-investigation.md`, this index | Complete; historical executor stall not reproducible |
| 3. Decisions/ADRs | Eight decisions recorded; Git initialized on `main`; published ACP 2.0.0 establishes Rust 1.88 floor; five target families confirmed | `decision-register.md`, `adr/0001`–`0008`, this index | Complete; product approvals remain explicit |
| 4. Fixture charter/schema | Versioned manifest/result JSON Schemas and normalization/security contract created | `fixture-charter.md`, `fixtures/{README,AGENTS}.md`, `fixtures/schema/*`, this index | Complete; runtime validation follows with oracle |
| 5. Python oracle/corpus | 65 scenarios across 7 categories; 132 schema-validated payloads; canary clean; stable index `27e58c…632f86`; source cancellation leak captured | `python-oracle-report.md`, `tools/python-oracle/*`, `fixtures/*`, this index | Complete locally |
| 6. ACP/SSE Rust spikes | ACP SDK 2.0.0/wire-v1 7/7 pass with wrapper requirements; reqwest 0.13.4 bounded SSE 10/10 pass with exact cancellation/partial-output rules | both spike reports and spike packages, this index | Complete locally |
| 7. SQLite/process/sandbox | rusqlite 0.40.1 bundled 6/6 and local system 6/6 pass; process conformance 9/9 and Linux Bubblewrap 3/3 pass; five-target workflow/scripts prepared | SQLite/process reports, both spike packages, CI workflow, this index | Complete locally; non-Linux/ARM64 CI pending |
| 8. Readiness audit | 65 scenarios and 132 hashes revalidated; all four local Rust spikes rerun; formatting/YAML/shell syntax checked; source identity/status unchanged | `blocker-closure-report.md`, this index | Complete; product approvals pending |

## Commands executed

1. `sed -n '1,240p' /home/alex/.agents/skills/docs-context7-first/SKILL.md`
2. `wc -l AGENTS.md docs/AGENTS.md docs/recon/AGENTS.md docs/recon/*.md`
3. Bounded `sed -n '1,999p'` reads of `AGENTS.md`, `docs/AGENTS.md`, `docs/recon/AGENTS.md`, and every Markdown report under `docs/recon/`.
4. Target identity/content/toolchain inspection: `pwd`; Git probes; `rg --files`; bounded `find`; `rustc --version`; `cargo --version`; `python3 --version`.
5. Source identity/environment inspection: `pwd`; Git root/remote/branch/HEAD/log/status; `.venv/bin/python3 --version`; `uv --version`; package/test searches; SHA-256 of `uv.lock` and `pyproject.toml`.
6. Focused config-switch diagnostic under five isolated state roots with `timeout 45`, `faulthandler.dump_traceback_later(8, repeat=True)`, and no pytest cache: 1 passed in 1.43s.
7. `tests/test_agent.py` under isolated state and `timeout 300`: 208 passed in 6.13s.
8. Focused historical-environment comparison with `timeout 60`: 1 passed in 1.40s.
9. Full `tests/` under isolated state and `timeout 900`: 879 passed in 89.41s.
10. Post-suite process enumeration plus source HEAD/status and isolated-state inventory.
11. Context7 resolution/queries for official ACP Rust SDK, rusqlite, and reqwest documentation.
12. Current primary package metadata via `cargo search/info`, local rustup state, and source release-target searches.
13. `git init -b main`; target branch/status verification; no commit or remote.
14. Oracle module compilation and iterative bounded captures against the frozen source.
15. Two deterministic process-subset recaptures; complete fixture index matched byte-for-byte at SHA-256 `27e58c39fe95882961bf877b132b4ecbc6209850c57cd801fc2219e345632f86`.
16. `oracle.py validate-all` (65 scenarios) and `verify-index` (132 payload hashes); category counts and source process observations inspected.
17. ACP Context7/current package reconciliation; downloaded crate/schema/example/ordering inspection; exact-pinned `cargo fetch`.
18. ACP disposable spike `cargo test --locked`: 7 passed, 0 failed.
19. Rust SSE exact-pin resolution plus initial/final `cargo test --locked`: final 10 passed, 0 failed.
20. rusqlite package/feature and local SQLite probes; bundled `cargo test --locked` 6/6; system-feature test 6/6; feature and debug-binary size inspection.
21. Linux primitive probes: `command -v bwrap`; `bwrap --version`; `uname -a`; user-namespace sysctl; `unshare --user --map-root-user --pid --fork --mount-proc true`; bounded Bubblewrap PID/network tests.
22. Target/source searches for process, sandbox, Job Object, Seatbelt, process-group, and cancellation symbols using `git ls-files`, `rg`, and bounded numbered source reads.
23. Process spike dependency resolution and iterative `cargo test --locked`; the initial pinned `libc` conflict was corrected, a namespace assertion was corrected from host-backed `/sys` to `/proc/net/dev`, and an interrupted pipe-holder diagnostic was bounded and cleaned.
24. Explicit `kill 65198` of the sole synthetic fixture child left by the interrupted diagnostic, followed by survival and later process enumerations.
25. Final process spike `timeout 90s cargo test --locked`: process conformance 9/9, Linux Bubblewrap 3/3, no failures.
26. Official GitHub-hosted runner-label lookup from GitHub documentation; workflow matrix created for the five release-target families.
27. Final `oracle.py validate-all` (65) and `verify-index` (132), plus category-count calculation.
28. Final local spike matrix: exact-locked ACP 7/7, SSE 10/10, bundled SQLite 6/6, process 12/12, and system SQLite 6/6.
29. `cargo fmt` followed by `cargo fmt --check` for all four spikes; workflow parsed with PyYAML; macOS shell script parsed with `sh -n`; local `pwsh` availability probe (unavailable).
30. Completion searches for status markers/placeholders, fixture/report file counts, target Git status, source HEAD/status, and matching descendant processes.
31. Post-format exact-locked rerun of all local spike configurations; all passed. Final source HEAD remained `bf4d428…20045`, tracked diff count zero, and status retained only the pre-existing untracked roadmap prompt.
32. `cargo clean --manifest-path` for each disposable spike removed about 1.7 GiB of ignored build output without touching source or authored spike files.
33. Required-deliverable existence audit, incomplete-marker search, final source status, and process enumeration: no missing deliverables, incomplete markers, or fixture/oracle descendants.

## VRO-14 production completion acceptance — 2026-09-07

- Baseline `v0.20.88` / `9ff4695b`; requirement-to-source map and exact
  commands/results: `vro14-gap-audit.md`.
- Implemented real contained pipe-CDP sessions, render waterfall, bounded
  sitemap/gzip discovery, shared engine configuration and pinned driver build.
- Canonical verification passed; MSRV 1.88: 1,673 passed, 0 failed, 20 ignored
  across 87 suites; both unchanged release-profile performance gates passed.
- Immutable-image real browser and explicit navigation/chunked-fetch tests
  passed on Linux x86_64; no provider calls or user-state writes; post-test
  container enumeration empty. Two native image CI jobs gate public assets.
- DOX pass updated web/config/harness/sandbox, fixture, workflow and owning
  documentation contracts. Version-only crate contracts remain unchanged.
- Released `v0.20.89` at `5658da6eefa8a13042e938eaedccfdb7a1537ad5` after
  exact-commit canonical/supply-chain, MSRV, five-target and both driver-image
  jobs passed. Release run `34072500086` passed; all 16 public assets and seven
  archive digests verified, Linux binary version smoke passed, and the exact
  published x86_64 image passed both real-browser tests. Registry PR #539
  updated in place at fork commit `4f62da58cc37d6c77425218cc80fdc49db26f920`.

## VRO-15 independent acceptance audit — f662519

- `vro15-gap-audit.md` rejects the full-completion claim for implementation
  `fb14ea2`: 18 grouped findings with source evidence and proposed repairs.
  Production fixes await Alex's approval; default-off behavior remains unchanged.
- Reproduced workspace floors: 1,893 all-features / 1,869 default, zero failures.
  Actual swarm suite: 163 passing tests (also on Rust 1.88), not the ADR's 171.
- `vro15-audit-probes.rs`: 16 standalone probes reproduce defects in cancellation,
  pool bounds/replacement, topology failover, sandbox sharing/teardown, bus lifecycle
  and HNSW loading/determinism. Passing these probes confirms defects, not acceptance.
- Architecture, naming guard, workspace formatting and swarm Clippy pass. Full
  canonical/supply-chain/target-matrix acceptance is not certified by this audit.
- Treat the following original closeout as historical claims, not current acceptance.

## VRO-15 original closeout claims — 2026-09-10

- The original closeout described `vesper-swarm` as delivered in ten PRs (scaffold+guard, topology
  manager, worker pool, priority bus, assignment/timeout, HNSW core,
  hybrid ledger, sandbox leases, hive orchestrator + adapter,
  documentation closeout); decision record is
  `docs/adr/0025-provider-neutral-swarm-orchestration.md`, requirements
  and per-PR evidence live in `docs/swarm-oracle-extraction-prd.md`.
- Measured floors at close: **1,893 passed / 0 failed**
  (`cargo test --workspace --all-features`) and **1,869 / 0** (default
  features — the zero-degradation proof; the default build links no
  swarm symbols). Monotonic ladder across PRs: 1,741 → 1,767 → 1,785 →
  1,813 → 1,832 → 1,853 → 1,868 → 1,881 → 1,893; no test deleted or
  weakened.
- Quality bars with executable evidence: HNSW Recall@10 = 0.997 @ ef=16
  and 1.000 @ ef≥64 against brute-force cosine on 10k seeded vectors
  (`crates/vesper-swarm/tests/hnsw_tests.rs`); sandbox teardown survives
  holder panics with exact acquire/release pairing
  (`crates/vesper-swarm/tests/sandbox_tests.rs`); the naming embargo is
  a CI ratchet (`cargo xtask naming-guard`,
  `xtask/naming-guard-baseline.json`: 30 frozen pre-existing hits,
  0 new).
- `cargo xtask architecture` validates the feature-gated edges
  (`vesper-harness → vesper-provider/vesper-swarm` as optional deps
  only); `cargo xtask verify` runs the naming guard in CI.

## Outstanding acceptance

- Registry PR #539 awaits upstream review; the v0.20.89 release and web
  implementation acceptance are complete (`vro14-gap-audit.md`).

- Historical executor-stall cause is not reconstructable, but the complete source baseline is green and repeatable.
- Product approvals listed in `decision-register.md` and `blocker-closure-report.md`.
- ACP `PromptResponse.userMessageId` compatibility-wrapper detail during Stage 1.
- Linux ARM64, macOS Intel/Apple Silicon, and Windows x86-64 workflow execution.

## VRO-15 concurrent dispatch and embedding boundaries

- `crates/vesper-swarm/tests/hive_concurrency_regressions.rs`: three-party barrier
  overlap, prerequisite ordering, sibling cancellation and aliased-port capacity.
- `hive_boundary_regressions.rs`: hanging embedding is bounded, publishes nothing,
  retains the interrupted goal and refuses replay.
- Source/limits: `vro15-repair-execution.md`, concurrent-dispatch milestone.
  Native host, worker factory and lease/health acceptance are not implied.

## VRO-15 lease resource bounds

- `sandbox_tests.rs`: specification/timeout/boundary bounds refuse before backend
  work; 4,096 waiter overflow leaves state unchanged; dropped waiters reclaim the
  queue; shared joins cannot bypass 4,096 global members.
- `sandbox.rs` also bounds retained diagnostic bytes and guards identity exhaustion.
  Synchronous port preemption and real supervisor lifecycle acceptance remain open.

## VRO-15 local verification after concurrent waves and lease bounds

- `cargo test --workspace --all-features`: **1,954 passed, 0 failed, 21 ignored**.
- Workspace all-target/all-feature Clippy and Rust 1.88 locked compilation,
  formatting, architecture, naming and whitespace checks pass.
- Seven added regressions; feature remains default-off. Independent pool creation,
  supervisor/host integration and final audit acceptance remain explicitly open.

## VRO-15 independent factory milestone

- `pool_instance_regressions.rs`: seven passing tests for independent boot/turn
  overlap, growth/replacement/shrink, rollback, timeout/close/drop cancellation,
  alias refusal and failed-lease quarantine.
- `swarm_adapter_tests.rs`: seven passing adapter tests including native factory
  pooling followed by real tool execution and provider continuation.
- Hive per-instance lifecycle/lease/health wiring and host acceptance remain open;
  see `vro15-repair-execution.md`. No full F01/F07 completion claim is made.

## VRO-15 factory verification checkpoint

- Workspace all-feature tests: **1,962 passed, 0 failed, 21 ignored**.
- All-target/all-feature Clippy and Rust 1.88 locked compilation, formatting,
  architecture, naming guard and whitespace checks pass.
- Replacement physical-resource teardown remains a lease-composition gap;
  transactional pool publication does not prove supervisor capacity bounds.

## VRO-15 exact selected-lease execution

- `pool_selected_lease_regressions.rs`: three passing tests for exact selected
  identity, foreign/failed lease refusal and capability/deadline rejection.
- `run_task` and `run_leased_task` share the same execution boundary. Hive still
  needs concrete pool worker routing/provenance and lease/health/shutdown wiring.

- Selected-lease checkpoint: workspace **1,965 passed, 0 failed, 21 ignored**;
  all-target/all-feature Clippy and Rust 1.88 locked compilation, formatting,
  architecture, naming and whitespace checks pass. Full acceptance remains open.

## VRO-15 directed dispatch, bus correlation and source-captured scorer

- Disconnected dispatch and identical-prompt bus substitution regressions failed
  before repair. Candidate routing now follows live directed topology paths from
  the elected navigator; dispatch checks its exact message ID and concrete peers.
  Native three-session tool/synthesis integration still passes all four topologies.
- Seventy-two scoring outputs captured from the pinned oracle method match Rust;
  commit, method hash, type-match mapping and capture recipe are recorded in
  `vro15-repair-execution.md`. Oracle checkout remains unchanged and clean.
- Final canonical and Rust 1.88 workspace: **2,031/0/23**; default **1,994/0/17**.
  Scoped asynchronous leases, native Settings/hosts and external isolation/target
  acceptance remain open; no activation claim changes.

## VRO-15 backend teardown, identity and governance continuation

- Two backend regressions reproduced false-success teardown; explicit namespace/
  Docker cleanup now reports ownership, status and reaping failures. Post-deadline
  reaping is polled for at most 500 ms. Actual isolation remains platform-gated.
- Three lease identity regressions reproduced duplicate active/queued/quarantined
  admission; typed refusal now precedes backend/queue mutation.
- Strict counted JSON naming baseline preserves all 30 HEAD-frozen exceptions;
  four self-tests enforce stable line shifts without permitting new duplicates.
  ADR 0026 supersedes original completion/adapter/ACP-exclusion claims; current
  migration and PRD status correctly retain default-off repair status.
- ETXTBSY fixture failure reproduced; immutable checked-in CLI with isolated state
  replaces runtime-written executables. Twenty repeated fixture suites pass.
- Canonical and Rust 1.88 full workspace **2,027/0/23**, default **1,990/0/17**.
  Six namespace test bodies skip locally; three Docker integration tests are ignored.
  Detailed methods/limits and remaining acceptance: `vro15-repair-execution.md`.

## VRO-15 native composition and shared sandbox outcome acceptance

- Shared TUI/ACP sandbox adapter now reports teardown failure, retains run
  diagnostics and quarantines subsequent provisioning. Six outcome/refusal unit
  tests pass; actual supervisor failure/panic/hang acceptance remains open.
- Native Hive integration covers three barrier-overlapping isolated provider
  sessions, real read-file tool continuations, evidence-fed synthesis and no
  implicit durable state under all four topology configurations.
- Canonical offline verification and full Rust 1.88 tests pass **2,016/0/23**;
  default workspace **1,981/0/17**. Supply-chain gates and real Chrome interview
  browser test pass. One earlier MSRV driver-fixture spawn failure did not recur;
  OS diagnostics improved but its root cause remains unproven.
- `vro15-repair-execution.md` now contains the current F01–F18 reconciliation,
  exact acceptance limits and remaining native host/supervisor/platform work.
  Full repair is still in progress; activation remains default-off.

## VRO-15 explicit quarantine recovery

- `LeaseBook::retry_quarantined` adds bounded caller-owned cleanup through an
  explicitly supported idempotent backend port; unsupported/failed attempts keep
  capacity quarantined. Three offline regressions cover handoff, accounting,
  shared names, budgets and closed-book recovery.
- Package verification: **260 passed, 0 failed, 3 ignored**; strict package
  Clippy, Rust 1.88 locked all-target check, architecture, naming and whitespace
  checks pass. Native supervisor cleanup and host activation remain unaccepted;
  details and limitations are in `vro15-repair-execution.md`.

## VRO-15 concrete Hive pool routing

- Six `hive_pool_regressions.rs` tests: actual 1+3 pool overlap and executing-ID
  provenance, cross-role alias refusal, nominal-slot refusal, actual cancellation,
  notification ordering and transactional/idempotent topology admission.
- `Hive::with_factories` uses concrete pool instances; `Hive::new` no longer
  fabricates several topology workers from a single supplied port.
- Health-driven lifecycle changes, verified leases/teardown and native host
  acceptance remain open; details are in `vro15-repair-execution.md`.

- Concrete Hive routing checkpoint: workspace **1,971 passed, 0 failed, 21 ignored**;
  all-target/all-feature Clippy and Rust 1.88 locked compilation, formatting,
  architecture, naming guard and whitespace checks pass. Native host/full audit
  acceptance remains open; activation stays default-off.

## VRO-15 native hosts and shared scopes (active working tree)

`vro15-repair-execution.md` records local ACP transport and TUI command/task/history
acceptance, real configured loopback embeddings, native tool continuations, both
scope modes, shared-container sibling confinement and descendant cleanup, original
timestamps, portable snapshots and repaired clustered HNSW recall. Canonical/MSRV each pass 2072 tests; default workspace passes 2015. All four
optimized scale tests and final audit/deny pass. External exact-commit target and
namespace gates remain separately tracked for v0.21.6; v0.21.5 CI does not certify
this implementation. The TUI fixture's rejected synthetic-key public
request is recorded explicitly and excluded from acceptance; corrected runs assert
the configured loopback endpoint before dispatch.


## VRO-15 exact-commit supervisor follow-up

Initial candidate CI exposed the post-unshare overflow-ID defect, rootful Docker
artifact ownership mismatch and noncanonical macOS/Windows fixture roots. The
repair ledger and ADR 0027 record fixes, including private-root confinement,
capability drops, bounded pipe/handshake waits and truthful nonzero shell exits.
Real local namespace Hive and the explicit security/timeout gate now pass, as does
shared Podman confinement and original-owner restoration. Candidate CI success
must be re-established on the final commit before tagging; no release was used to
discover these failures.


## VRO-15 final implementation and release evidence

The final F01–F18 matrix records completed implementation and local acceptance,
including the corrected namespace supervisor. Canonical/MSRV: 2072 passed, zero
failed, 34 explicit ignored bodies; default: 2015/0/20. The required real namespace
and shared-container gates execute separately. Candidate `e6476df` has successful
canonical, MSRV and both-architecture complete web-driver workflows; its Windows
platform interruption was an HTTP 500 cache download before tests. Optional cache
setup now reaches the existing direct-compilation fallback. Final publication still
requires all four successful push workflows on the exact release commit. The
v0.21.6 release notes own the final run links, image IDs and local-install receipt.

## Native completion assurance implementation — 2026-09-11

- Approved ADR 0028 and `completion-assurance-execution.md` own the native gate,
  adversarial/real-repair cases, exact-case CI and bounded mutation evidence.
- Final repository verification and release readiness are reported there;
  research references alone do not establish implementation completion.
- Final canonical verification and both native binary builds passed. All 20
  exact acceptance cases passed; both deliberate evaluator mutations were caught.
  Local implementation evidence does not imply a release or installed update.

## VRO-16 release v0.21.9 — 2026-09-12

- Tag `v0.21.9` at `0b4cd0d`. The first version commit `b790e4f` was
  rejected by the web-driver exact-commit gate: the native loopback
  providers scripted only pre-governance turn shapes, so the composed
  review-panel and decision turns failed inside the native
  shared-service/ACP/TUI gates. This is the exact-commit contract
  working as designed — the gap was invisible to local canonical gates
  because the native container/service fixtures run only in the
  web-driver workflow's environment.
- Repair: `TaskKind::Review` maps to an empty tool registry in the
  provider adapter (D3 — judges evaluate, never author/re-execute),
  native fixtures answer review and decision turns, count expectations
  updated, cancellation invariant made baseline-relative. All gates
  reproduced locally with the real CI-built driver image (podman) and
  the release supervisor before pushing the fix.
- Green on the exact commit before tagging: canonical, MSRV,
  five-target foundation, dual-architecture web-driver (run ids
  34674795540/34674795595/34674795554/34674795541). Release run
  34676270024 verified exact-commit CI and published 16 assets;
  checksums verified locally; installer upgrade preserves user state;
  registry PR #539 updated in place.

## VRO-16 advanced hive governance — 2026-09-12

- Requirements: `docs/advanced-hive-governance-prd.md` (PR-1 §2, PR-2 §3,
  PR-3 §4). Upstreams referenced exclusively as governance alpha/beta
  (recon `docs/architecture/recon_vro16_governance.md`, pinned commits).
- Final audit found nine integration gaps (G1–G9) between the tested
  engines and the composed production path — the VRO-15 decorative-layer
  failure mode. All were repaired in the same audit pass and each now has
  an executing proof in
  `crates/vesper-swarm/tests/hive_governance_audit_fixes.rs`.
- G2 gate bus publication is real: the `governor` inbox subscribes at
  topology admission (`MessageKind::Governance`, Urgent tier); gate sends
  fail loudly on publication failure; `drain_governance_bus` is the host
  observation seam.
- G3 ledger audit trail is complete: every gate resolution persists
  before its state effects (Cancel no longer drops its own audit record);
  `record_gate_event` derives the goal from the event, and the audit
  entry round-trips as an `AuditEvent` through `EntryKind::Audit`.
- G4 `governance: gated` enforces decomposition and synthesis boundary
  gates (once per task id per run — `resolved_gate_ids`).
- G5 Redirect directives reach re-dispatched worker prompts.
- G9 SmartPause consumes the real watchdog percentage
  (`BudgetWatchdog::percent_consumed`), not a constant.
- G1 the review panel composes pre-synthesis on governance-enabled hives
  (async round driver over driver ports; bare VRO-15 hives keep exact
  turn-count contracts; zero-panel fails closed).
- G6 budget exhaustion renders in both hosts; G7 the directive rides
  `GateResolved.command` (PRD D4 amended instead of a redundant variant);
  G8 this entry.
- **G4 re-audit (same day, prompted by direct owner challenge):** the
  boundary-gate fix above was itself incomplete — gates opened but did
  not pause the gated work (the decomposition gate fell through to task
  dispatch in the same tick; the synthesis gate ran panel+synthesis
  anyway), the G4 proof asserted only gate *views*, not held-back work,
  and `run_to_completion` could hot-loop on an unresolvable open gate.
  All three repaired: both boundaries now park the run (`Ok(false)`,
  `active_goal` cleared, assignments/evidence retained) and resume after
  host resolution or expiry; `run_to_completion` returns instead of
  spinning when a gate is open; the proof is behavior-based — zero task
  prompts reach drivers while the decomposition gate is open, synthesis
  is held until its gate resolves, the goal completes after both
  resolutions, and the spin-safety case asserts the run stays parked.
  Lesson recorded: a gate that does not stop anything is not a gate, and
  proving "the gate exists" is not proving "the gate gates."
- Final verification: workspace all-features and default suites, strict
  Clippy, fmt, architecture (27 packages), naming-guard (11 tokens, 18
  frozen hits, zero growth), `cargo xtask acceptance` 20/20. Counts in
  `docs/migration-status.md` VRO-16 row.

- `v0.22.1-release-execution.md` — exact-commit release of the A1 audit fix: 4 workflows green on 1b221dc, tag v0.22.1, 16 assets, binary 0.22.1, registry PR #539 head 8138e28, local install updated (both binaries 0.22.1).
- `ranker-hardening-final-audit.md` — cross-PR audit: scope fence verified (2-line diff), pin sabotage-verified bidirectionally, A1 latent metric defect found+fixed (overhead tokenizer vs ranker pool; shipped numbers unaffected), M2 re-derived, release evidence reconciled.
- `ranker-hardening-pr3-execution.md` — stop rule clean (canonical table + overheads identical under the hardened ranker; verdict re-derived ADOPT); v0.22.0 released per the exact-commit contract (4 workflows green on 9b56a9e; tag; 16 assets; binary 0.22.0; registry PR #539 updated in place).
- `ranker-hardening-pr2-execution.md` — Option A decoupling: chunk pools raw-stemmed (`raw_semantic_tokens`), prompt pool keeps expansion; pin green, control green, D3 ladder green, skill tier frozen, floor 2,220.
- `chunk-score-floor-pr1-execution.md` — noise anchor: offline-exact ranker replication found a 1,440-pt pure-cosine disjoint pair (0.6547, zero overlap, no name-match); pin test fails on the unhardened tree with receipt, `#[ignore]`d until PR-2; genuine-overlap control green (+1 floor test, 2,253); clippy 0, acceptance 23/23, naming-guard clean. (Self-caught write_file-replaces-instead-of-appends near-miss recorded in §5.)
- `chunk-score-floor-prd-execution.md` — PRD for the chunk-tier cosine noise floor: conjunction gate `(overlap>=1 || name_match) && score>=520` in `rank_chunks` (bare 520 floor rejected — observed 678-pt noise outlier passes it; sensitivity table in PRD §1.1); 3-PR anchor-first plan; G5 literal restoration in PR-3. DOX updated (AGENTS.md bullet, migration-status PLANNING).
- `exemplar-migration-execution.md` — first native chunked skill: `research-paper-writing` 74,109 → 10,228 B lean body + 16 chunks (67,426 B, max 11,125); Stage D proofs as real tests (16/16 routing-first, budget ≤24 K, G5 with recorded hash-noise finding: zero-overlap prompts can still route ≤3 chunks on positive signed-hash cosine — floor-remediation flagged for a future PRD); floor 2,252, acceptance 23/23, clippy 0, naming-guard clean; global mirror synced.
- `recon-exemplar-candidate-execution.md` — exemplar hunt (read-only): `research-paper-writing` selected (74,109 B / 27 sections / body 3.1× the 24 K injection cap — 20 of 27 sections never injected; ~1.3 MB shadow reference layer); 16-chunk blueprint, lean ~6.1 KB body, worst-case activation ~19 KB.
- `ranker-hardening-pr1-execution.md` — Option D anchor: cross-talk pin fails for the audited reason (rollback displaces migrate via 1,560-pt manufactured overlap); no-alias control green; floor 2,219; `#[ignore]` until PR-2.

- [Routing request recognition](skill-routing-request-recognition-execution.md): ongoing sourced-verb and topic-evidence experiments; retains negative-activation regressions and pending independent evaluation.

- [v0.22.6 release execution](v0.22.6-release-execution.md) — voice controls, long dictation and exact-commit publication.

[Increment 37 — v0.23.1 PUBLISHED](../foundation/v0.23.1-release-final.md): Bridge shipped experimental/default-off on 5 platforms (14 checksummed artifacts); one registry-manifest gate failure caught + fixed + re-tagged; registry PR #539 updated in place per continuous-update contract. Live: github.com/99percentgrip/agent-vesper/releases/tag/v0.23.1
[Increment 38 — v0.23.3: Bridge actually IN the binaries + native Settings activation](v0.23.3-release-execution.md): v0.23.1 artifacts contained ZERO Bridge code (release.yml built only docker,swarm); release matrix now compiles bridge into both hosts (byte-grep receipts). Native activation: TUI `/settings → Bridge` panel + ACP `/settings bridge` text controls (BR-21 parity); `/bridge` disabled text points to Settings, never hand-edited JSON. Found + fixed a VACUOUS AT-01 pass: the old assertion matched the JSON snippet embedded in the disabled message; child cwd now pinned to the isolated root. Gates: fmt/clippy(both toolchains)/arch 28/naming 33/acceptance 23/23; suites harness 155+122, ACP 84+83, TUI 395, bridge 78 — all 0 failed.

## MCP conversation lifecycle repair

- [Execution report](mcp-session-lifecycle-repair.md): conversation-owned stdio,
  fault quarantine, host gateway wiring and isolated real-browser acceptance.
  Source repair only; installed-binary and cross-platform acceptance remain separate.
