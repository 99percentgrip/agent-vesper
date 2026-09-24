# Voice CPU production acceptance: application-loop regression, policy parity repair, and optimized-candidate verification

> **Scope:** VRO-17 continuation of the [capability-gated execution](voice-capability-gated-execution.md)
> unit. This work unit verifies the **running CPU voice application** through its
> real event loop, repairs the two defects the loop check exposed, and delivers a
> profile-matched optimized candidate with stage-distinct timing evidence.
> It is not NPU work: the accelerator registry stays honestly empty, and no
> device/microphone/speaker/live-provider evidence is claimed.

## Objective

Prove — not assume — that the optimized CPU voice candidate behaves correctly
through the actual application paths (native Settings save → F9 capture → shared
STT final → ordinary runtime submission → assistant deltas → hygiene → CPU
Kokoro → playback boundary → settlement), that Preview and F9 share one
execution-policy semantics, that interruption/error/lifecycle behavior is
bounded and recoverable, and that the artifact offered to Alex is the same
optimized build the measurements came from.

## Candidate and environment (recorded first)

```text
commit:  8f258ba28f4ea2f749526fb32b5b180ecc7dcead (HEAD, pre-existing dirty workspace;
         88 entries before this unit — this unit's files listed below)
toolchain: rustc 1.95.0 (59807616e 2026-04-14) / cargo 1.95.0 (f2d3ce0bd 2026-03-21)
disk:    245 GiB free at start (1021431513088 B total, 263248728064 B free); 1 GiB reserve held
assets:  installed Natural Voice pack ~/.local/share/agent-vesper/voice-pack
         (118009413 B ≈ 112.5 MiB, unmodified; assets hard-linked read-only into
         PTY fixtures — never copied, never re-downloaded)
python:  ~/.local/share/agent-vesper/voice-venv/bin/python (verified present)
build:   cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui --release
binary:  target/release/agent-vesper-tui
sha256:  d633b4c370aafe9a4f49b08aac3e19dd7693efeb437a95a8190dd0b3b5dc6572 (22388680 B)
model/voice (tests): Kokoro model_quantized.onnx / am_michael; provider = the
         suite's fixed loopback LM-Studio fixture (never a real provider)
reasoning modes exercised: off (direct) and balanced (VRO route)
```

The installed application, sessions, and settings were not modified; test
configuration used isolated roots with the approved CPU STT/TTS paths.

## §2 result: Preview/F9 policy parity — defect found and repaired (red→green)

Source inspection established two defects before any fix:

### Defect 1 (semantic): Preview ignored the TTS execution policy

The Natural Voice pack screen spawned its Preview `SpeechWorker` from pack
presence alone, while the F9 gate (`main.rs::conversation_gate_decision`)
refused a strict-NPU scope with a stage-specific refusal. With a saved/copied
`tts_compute = "npu"` scope, F9 refused but Preview synthesized on CPU — one
effective configuration, two policy outcomes, violating the single-selection
rule and the no-relabeling guarantee.

**Red receipt (pre-fix tree, verbatim):**

```text
error[E0425]: cannot find function `preview_policy_gate` in module
`agent_vesper_tui::voice_accel`  (tests/voice_policy_parity.rs:176/197/227)
```

**Fix:** `voice_accel::preview_policy_gate(scope)` resolves the TTS stage
through the SAME shared rule (`stage_route_for_policy`) the F9 gate uses,
against the effective configuration in the Preview context (the visible
Settings **draft** — the draft/saved distinction is preserved; Preview never
saves). The pack screen consults it before offering the Preview row and
surfaces the exact refusal in the screen notice (never silent hiding, never a
setup prompt for absent hardware). No new registry, flag, cache, or scheduler.

### Defect 2 (resource): no-op Settings save rebuilt the acoustic engine

`ConversationHost::reload_engine_selection` unconditionally sent
`Command::Replace`, bumping the speech generation (invalidating queued speech)
and rebuilding the engine even when engine+voice were unchanged.

**Red receipt (pre-fix tree, verbatim):**

```text
test unchanged_engine_reload_keeps_generation_and_engine ... FAILED
assertion `left == right` failed: a no-op reload must not bump the speech generation
  left: 0
 right: 1
```

**Fix:** reload compares the resolved selection first; an unchanged selection
sends no `Replace` (no generation bump, no engine rebuild). A real change
still swaps at the same safe unit boundary — kept green by
`changed_voice_still_replaces_at_the_safe_boundary` (generation +1 exactly).

### §2 behavior matrix (all through real host seams)

| Effective policy/state | Result | Evidence |
|---|---|---|
| CPU | CPU route; zero accelerator consultation (structurally skipped) | `cpu_policy_is_accepted_by_both_paths`; `voice_execution_policy` zero-call test |
| Automatic, empty registry | Ordinary CPU outcome, truthful reason, no vendor tool/download | `automatic_policy_is_ordinary_cpu_for_both_paths` |
| Saved strict-NPU, no route | Stage-specific refusal naming blocker + Settings remedy, IDENTICAL for F9 and Preview; option hidden from ordinary selection | `preview_resolves_the_same_tts_policy_as_the_f9_gate`, `pack_screen_consults_the_tts_policy_before_preview` |
| Unconfigured | Gate Disabled; no voice initialization; empty registry | `unconfigured_scope_stays_disabled_and_initializes_nothing` |

Already-prepared workers: unchanged policy → engine retained; policy/voice
change → re-resolve at the unit boundary with queued-speech invalidation
(`replace_selection_invalidates_queued_jobs`). STT/TTS stay independent;
Preview initializes no STT.

## §3 result: the actual application loop — and a second defect it caught

**Isolation verified before running** (`tests/settings_pty.py` + `r3_loop_pty.py`):
controlled HOME/XDG/workspace roots; blackhole proxies `127.0.0.1:9` with the
loopback provider the only `NO_PROXY` exception; deterministic recorder/STT/player
doubles on a private PATH; verified-pack assets hard-linked (no copy, no download);
child in its own process group, SIGTERM+wait ownership; no credentials, devices,
services, or developer-global state.

The chain exercised is the real one: native Settings/`config.toml` → F9
capture boundary → shared STT final → ordinary runtime submission (the
provider-wire instruction check proves the production path) → hygiene gates →
real cached Kokoro adapter → fixture player sink → settlement. No injected
AgentEvents; no controller shortcuts.

### Defect 3 (functional): pack-screen action off-by-one — Preview fired Repair

The first post-§2 PTY Preview run failed:

```text
AssertionError: Preview did not expose the actual synthesis stage
(screen dump shows the "Repair / Verify voice pack" result dialog)
```

My §2 rewrite computed action indexes from a zero-based list while the menu's
row 0 is the status row, so clicking "Preview voice" selected **Repair**.
Exactly the failure mode the production-loop check exists to catch.

**Fix:** rows and action indexes now derive from ONE ordered `PackAction` list
(`action_index` adds the status-row offset), so they cannot disagree when
Preview is policy-hidden.

### PTY loop receipts (post-fix, final candidate)

```text
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off)
      -> fixture player; PCM bytes=[52000, 151200]; peaks=[22327, 17608];
      provider requests=1
PASS: F9 -> ... (balanced/VRO) ...; provider requests=1; 'VRO' route confirmed
PASS: actual Settings Preview -> real Kokoro -> fake player;
      click-to-first-PCM=1748.8 ms; peak=17758; no recorder/provider
r3_repro_pty.py (release binary): UI remained responsive; no speech-failed status
      (no provider configured in that isolated root — expected)
```

Exactly one submission per turn; ordered nonempty PCM with healthy peaks
(near-zero-signal regression clean); one terminal outcome; text answers
unchanged; no recorder or provider request from Preview.

## §4 result: interruption, error, and lifecycle regressions

New production-path suite `tests/voice_interruption_lifecycle.rs` (5/5 green;
drives the real `ConversationHost` + `SpeechWorker`, core `FakeStt`):

- **Five interruption cycles then a full turn**: exactly one submission and
  live synthesis dispatch every cycle; interrupted turns suppress later
  deltas/finals (no old-audio resumption); the post-cycle full turn submits and
  dispatches again (no Stop latch).
- **No transcript replay across five cycles** (exactly one transcript per capture).
- **Failure before audio**: STT `Unavailable` submits nothing, surfaces
  visibly, never dispatches speech; the next turn recovers fully.
- **Session exit teardown**: dropping the host leaves a later session on a
  clean lane (fresh worker, generation 0, its own full turn).
- **Preview worker lifetime**: one worker per pack screen; consecutive screen
  lifetimes independent (a dropped screen's job never settles on the next).

Existing production-path suites remain green and cover the remaining matrix
rows: stop-during-inference/queued-playback and stale-rejection
(`r3_speech_worker`, `voice_speech_pipeline`: stop returns <1 s, stale pieces
never start a player, lookahead bounded to one), five worker stop-cycles
(`worker_survives_repeated_stop_cycles`), engine-swap invalidation, playback
device-failure classification without raw stderr (`voice_playback_diagnostics`),
failure after partial output preserving text
(`synthesis_failure_preserves_text_answer_and_runtime_outcome`), deferred
cancellation on late runtime identity, and the one-interruption-note contract
(`pr4_wiring`). Runtime tool-replay safety remains the harness's transactional
regression evidence, not the no-tool checklist.

## §5 result: optimized application timings (stage-distinct, matched profile)

All numbers below are from the **release binary** (`d633b4c3…`), i.e. the same
optimized application offered to Alex — the debug/release mismatch from the
prior unit is closed. Machine: Alex's laptop, ordinary concurrent load; small
samples, reported as observed. Software-delivery receipts: first PCM reaching
the transport sink is **not** acoustic onset, and the loopback provider's
timings are controlled-test evidence, not predictions for Alex's real provider.

**Preview — actual UI click-to-first-PCM through the full PTY loop** (includes
menu rendering and event-loop polling; three attempts, each a fresh process):

```text
1748.8 ms / 2161.5 ms / 2870.2 ms   (median ≈ 2162 ms)
```

**Preview — warm reusable worker, enqueue-to-first-PCM** (release helper;
readiness pre-established, so this is the warm/ready floor, labeled as such):

```text
process A: 1.638 s, 1.562 s   process B: 2.493 s, 2.449 s   process C: 2.421 s, 2.432 s
worker readiness (cold, per process): 0.443 s
```

**F9 stop-to-first-PCM through the full loop** (includes synthetic STT with a
deliberate 2 s import + 1 s provider hold; NOT end-to-end latency):

```text
release: 2435.1 ms / 3176.7 ms / 3064.5 ms / 3082.0 ms  (median ≈ 3073 ms)
recorder-process onset: ~250 ms each (capture responsiveness preserved)
```

**Longer-response fixture** (188-byte unit, fixed text, no playback; first-PCM
by the fixture player, streaming continues to full settlement):

```text
1.715 s / 2.069 s / 2.687 s first PCM; full delivery ~9.9 s (247200 samples)
```

These sit within the previously recorded post-repair ranges (Preview warm
1.56–2.49 s; long-unit 1.62–2.59 s) with machine load visible in the spread.
Stage boundaries (capture → transcript visible → submission → first eligible
text → first safe speech unit → synthesis ready → first PCM → player drain)
remain the existing telemetry points; no new profiler or logging was added.
The historical ~20 s first-speech baseline remains superseded; no new numeric
acceptance threshold is invented here.

## §6: storage, tests, and final artifact

```text
disk before: 263248728064 B free  →  after: 262685802496 B free (build growth only)
preserved candidate: target/voice-candidates/agent-vesper-tui-voice-cpu-acceptance
  sha256 d633b4c370aafe9a4f49b08aac3e19dd7693efeb437a95a8190dd0b3b5dc6572 (22388680 B)
  (copy made so later default-feature builds cannot clobber the offered artifact;
   identity re-verified at handoff — same digest)
temp fixtures: all isolated under owned roots; 0 residue (verified after cleanup)
voice pack: unmodified (118009413 B; read-only hard links only)
no downloads, installs, cargo clean, or deletion of pre-existing data
```

### Gates

| Gate | Result |
|---|---|
| vesper-voice lib + suites | **passed** (85 / 13+29 / features 29+0 dup) |
| TUI voice lib (voice-kokoro) | **passed** (278) |
| TUI default lib (R9 parity) | **passed** (242) |
| voice_policy_parity (new) | **passed** (7/7; red receipts recorded above) |
| voice_interruption_lifecycle (new) | **passed** (5/5) |
| r3_speech_worker / voice_speech_pipeline / voice_playback_diagnostics | **passed** (6 / 1 / 2) |
| pr4_f9_gate / pr4_wiring / r3_voice_pack / voice_execution_policy | **passed** (10 / 10 / 10 / 10) |
| r3_loop_pty.py direct / VRO / Preview (release binary) | **passed** (3 modes) |
| clippy `-D warnings` (voice-kokoro, voice-conversation, vesper-voice all-features) | **passed** (exit 0) |
| cargo xtask architecture | **passed** (28 packages) |
| cargo xtask naming-guard | **passed** (33 frozen hits) |
| cargo xtask acceptance | **passed** (23 exact cases, 6675 ms) |
| rustfmt (all files changed by this unit) | **passed** |
| workspace `cargo fmt --all --check` | **not fully green — pre-existing `ui.rs` lines only** (documented, untouched) |
| docs link/whitespace check (docs/foundation) | **passed** (0 missing links incl. URL-encoded paths, 0 whitespace) |
| MSRV / five-target / CI / release workflows | **not run** (out of scope; remain for PR-5) |
| cargo deny (supply-chain) | **not run** (command not installed in this environment; previously green in CI) |
| NPU STT / NPU TTS / physical-device / live-provider checks | **not applicable to this CPU-only unit** (separate open gates) |

After the final production repair, the affected positive and negative
application-path tests were re-run on the final candidate (PTY Preview mode and
both policy suites above), and the release artifact digest was re-verified.

## Files changed by this unit

Production:

- `apps/agent-vesper-tui/src/voice_accel.rs` — `preview_policy_gate` (shared-rule Preview resolution)
- `apps/agent-vesper-tui/src/settings_host.rs` — pack-screen Preview gate + unified `PackAction` row/index derivation + refusal notice
- `apps/agent-vesper-tui/src/voice_conversation.rs` — no-change reload no-op (`speech_state` seam)
- `apps/agent-vesper-tui/src/voice_speech_worker.rs` — `generation()` accessor

Verification:

- `apps/agent-vesper-tui/tests/voice_policy_parity.rs` (new, 7 tests)
- `apps/agent-vesper-tui/tests/voice_interruption_lifecycle.rs` (new, 5 tests)
- `apps/agent-vesper-tui/Cargo.toml` (two test-target entries)

Docs: this report, `evidence-index.md`, `migration-status.md`,
`voice-user-latency-acceptance.md` (user checklist), `apps/agent-vesper-tui/AGENTS.md`.

## User-operated acceptance — prepared, NOT RUN (Alex-operated)

On the candidate build (`target/voice-candidates/agent-vesper-tui-voice-cpu-acceptance`,
or rebuild with the command above; identical digest):

- [ ] **Cold and warm Preview** (Settings → Voice → Natural Voice pack →
      Preview voice): click-to-first-audible-word; is the sound clear?
      (Cold = first Preview after opening the screen; warm = immediate repeat.)
- [ ] **Three F9 turns**: recording/Stop responsiveness, stop-to-visible-transcript,
      stop-to-first-audible-word; one request and one answer each.
- [ ] **One longer reply**: first word prompt; complete natural output; no long
      between-piece gaps or clipped words.
- [ ] **Explicit interruption then another spoken turn**: old speech stays
      stopped; the next response remains audible.
- [ ] **Failure clarity** (optional): automated controlled error-path evidence
      is in §4; any device-side check only if you deliberately choose a safe action.

Settings in effect: Voice enabled, Natural Voice (Kokoro), am_michael, CPU
compute for both stages (defaults). No settings were changed on Alex's behalf;
if your saved scope differs, adjust in Settings → Voice (draft + Save changes).

Launch:

```sh
cd /home/Alex/Projects/agent-vesper
./target/voice-candidates/agent-vesper-tui-voice-cpu-acceptance
```

## Verdicts (separate)

1. **Automated CPU application-loop acceptance: PASSED** for the tested scope —
   real event loop, three PTY modes, policy parity, interruption/error/lifecycle
   matrix, all through production paths with the real cached Kokoro adapter and
   deterministic external doubles.
2. **Measured CPU synthesis/software latency: recorded, not a product verdict** —
   release-profile receipts above; software-delivery floors, not acoustic onset;
   no new numeric threshold created.
3. **Alex's real-device acceptance: PENDING** — the checklist above is NOT RUN;
   automated PCM drains are not hearing evidence.
4. **Broader VRO-17/PR-5: OPEN** — quantitative user latency acceptance,
   repeated-interruption acceptance on device, NPU STT, NPU TTS, cross-platform,
   and PR-5 gates remain separately tracked and were not advanced here.
