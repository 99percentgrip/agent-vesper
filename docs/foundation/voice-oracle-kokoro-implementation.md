# VRO-17 R3: Lean Local Neural TTS — Natural Voice Pack Implementation

Status: **initial completion claim withdrawn after Alex's repeated live
failures. Production-routing repair built; isolated direct/VRO tests pass;
Alex's neural listening/F9 acceptance remains OPEN.** Section 13 is the current
execution report. Sections 1–12 retain historical implementation/setup evidence,
not proof that the reported live defect was fixed. PR-5 remains paused.

> **Supersession note (2026-09-23):** the "listening acceptance remains
> OPEN" line above was accurate at this record's date. Listening
> acceptance was subsequently recorded positive in later records — the
> PCM-scaling repair's confirmed clear playback, the 2026-03-03
> continuity listening acceptance, and the 2026-09-23 NPU-conversation
> acceptance (`voice-npu-user-acceptance.md`). This record's withdrawal
> of its *initial* completion claim stands as history; it does not
> describe the current acceptance state.

Baseline at unit start: `94ed16d` + the 42-path uncommitted VRO-17
tree (preserved; nothing unrelated was modified). Mission directive:
consolidated replacement (2026-09-19), authorizing bounded model/
runtime setup, the native installer, and local no-device verification
within stated budgets.

## 1. Product decisions (this unit)

| Decision | Choice | Evidence |
|---|---|---|
| R3 voice engine | Kokoro-82M via the ONNX-community export behind the pure core's `VoiceTts` port | audition record §3; directive authorization |
| Model file | `onnx/model_quantized.onnx` (92 361 116 B, sha256 `fbae9257…`) at revision `1939ad2a8e416c0acfeecc08a694d14ef25f2231` | **changed from the audition's q8f16 proposal**: q8f16 SIGSEGVs ORT 1.28.0 CPU at session creation (coredump: SEGV inside `libonnxruntime.so` graph optimization, called from `commit_from_file`; reproduced 3×). The directive pre-authorized the compact `model_quantized.onnx` alternative if mixed precision was unsuitable; +6.3 MiB retained |
| Inference runtime | ONNX Runtime **1.28.0** CPU, x86_64 Linux, official release archive (sha256 `a3e1b79d…`); loaded via `ort` 2.0.0-rc.13 `load-dynamic` — **no build-time link, no build-script download** | `ort-sys/build/download/verify.rs` reviewed; GitHub release digest captured; coredump above motivated the explicit CPU archive pin |
| Pronunciation | espeak-ng 1.52.0 (already installed, used in place, process boundary) producing IPA; the official kokoro-js pipeline's normalization/post-processing reimplemented in Rust (regex-free) | kokoro.js source reviewed (Apache-2.0); algorithm reimplemented, not ported |
| Voices | Heart (`af_heart`), Michael (`am_michael`) — the two the audition verified | pack manifest |
| Pack location | `~/.local/share/agent-vesper/voice-pack` (`$XDG_DATA_HOME` honored), one per-user cache shared across projects/updates | directive §3 |

Naming embargo: "kokoro"/"Kokoro" is upstream's model name used as
required attribution/provenance, not a forbidden stem (`xtask
naming-guard` passes with 36 frozen hits). Voice labels shown to users
are Heart/Michael.

## 2. Structure: what lives where

- **`crates/vesper-voice-kokoro` (new, dedicated adapter crate)** — the
  `vesper-web` → `vesper-web-fetch` precedent applied: the heavy backend
  stays out of the pure core. Modules: `pack` (descriptor, digests,
  record, integrity, removal), `phonemize` (normalization → espeak IPA →
  post-processing → IDs), `vocab` (115-char vocabulary parse), `engine`
  (ORT session, style packs, resample, validation), `adapter`
  (`VoiceTts` port wiring), `setup` (pipeline), `mock` (dev-only
  deterministic double). Depends on `vesper-domain`, `vesper-security`,
  `vesper-voice` only; `ort` behind the default-off `ort` feature with
  `default-features = false`, features exactly `std, load-dynamic,
  ndarray`. `#![forbid(unsafe_code)]`.
- **TUI feature `voice-kokoro` (new, implies `voice-conversation`)** —
  links the adapter only here. Default/`voice-conversation`-only builds
  compile none of it (default suite count unchanged: 241 lib / 154 bin).
- **No new registry/downloader/UI toolkit/state machine** — the pack
  flow reuses the Settings draft model, the `settings_menu` renderer,
  the `voice_readiness` shared assessment, the F9 gate, the
  `PlaybackOwner`, and the `SubprocessTtsConfig`-style bounded-child
  discipline (curl via fixed argv, like the harness's installer).

## 3. Pack integrity and lifecycle

- `pack.json` (format 1) carries revision, per-file digests, `verified`
  flag, voices, runtime version. Written atomically (temp+rename);
  `verified: true` is published **only after** digest validation of
  every component AND the bounded no-speaker synthesis probe.
- Digests are the authority: model/voices/vocab captured from the HF
  tree API at the pinned revision (LFS oids); the runtime library,
  license, and notices digests measured from the pinned archive's own
  extracted files (the archive is deleted after extraction and is never
  a retained/validated component — a defect found red and fixed).
- Readiness is cheap: `pack_identity` (sizes+mtimes) gates the heavy
  digest pass; no per-redraw hashing of the 86 MiB model, no per-redraw
  inference (directive §8).
- Setup stages (visible, determinate): Prepare → Vocab → Voices →
  Model → Runtime → Verify → Probe → Publish. Progress bytes are real
  file-size stats against pinned totals; Esc requests stop after the
  current step (curl is killed; the partial is kept and resumable via
  `--continue-at` against the immutable source; a digest mismatch
  discards the partial and prohibits cross-content resume).
- Capacity: plan gate refuses when free < peak budget (`df -B1`, the
  capture-store house pattern; unknown space defers rather than
  guesses). Backup/restore: an existing pack is renamed aside before
  install and restored byte-intact on any failure; partial staging can
  never read Ready.
- Removal: confirms the measured reclaim, deletes only pack-owned
  paths (plus now-empty owned dirs), preserves foreign files, refuses
  while a live foreign process holds an engine lease (`runtime/leases/
  <pid>.json`; PID-liveness checked), and explains that the selected
  voice becomes unavailable unless another engine is saved — no
  automatic baseline/cloud switch.

## 4. Pronunciation bridge (honest scope)

- Normalization follows the official pipeline's rule order (quotes/
  brackets → CJK punctuation → whitespace → Dr/Mr/Ms/Mrs/etc → yeah→
  ye'a → numbers/times/years/currency/decimals/ranges → uppercase-
  consonant possessive 'S / X'S → initialism hyphens). Punctuation
  sections pass through verbatim around espeak sections; «» (from
  parens) are dropped — the upstream tokenizer's own normalizer strips
  any character outside the 115-symbol vocabulary, and this
  implementation applies that defined behavior explicitly with a
  stripped-count report; input that strips to nothing is refused.
- espeak-ng is invoked `--ipa -q -v en-us --stdin`: bounded stdin/
  stdout, fixed argv, no shell, process-group-scoped kill-on-drop.
- Known limitation (recorded, not hidden): espeak IPA is not the full
  misaki G2P; unusual words/names may be mispronounced. The fixture
  corpus pins expected IDs for ordinary text; unknown-symbol behavior
  is the documented strip-with-count, never a silent drop.

## 5. Synthesis and playback (host contract preserved)

- The adapter implements `VoiceTts`: open-time `Unavailable` (pack
  absent/invalid, phonemizer missing) vs `Inference` (model failure on
  valid input); buffered-per-unit `streaming: false` (truthful);
  cancellation before the run returns `Cancelled`; mid-run cancellation
  is non-preemptible (ORT run completes, then the stream suppresses
  stale audio) — documented, with the post-cancel engine-usability
  check green. One engine per process, lazily built, invalidated on
  pack identity change or Settings save; never reloaded per sentence.
- Waveform contract validated per run: finite f32 samples, bounded
  length; 24 000 Hz is **converted** (bounded linear resample + i16
  clamp) to the canonical 16 kHz mono s16 — never relabeled.
- Playback stays the PR-4 `PlaybackOwner` (stdin pipes, no files);
  `speak_unit` now dispatches through `EngineHandle::{System, Neural}`
  selected from the saved scope. Speech failure keeps the text answer
  and the agent outcome (unchanged from PR-4).

## 6. User-visible surface (Settings → Voice)

- `Speech engine · current …` row → per-engine choice (System voice /
  Neural voice (Kokoro)); a voice-capable build only. Draft-only:
  selecting never installs; installing never selects; Save persists
  `tts`/`voice` into `[voice]` (save path extended; dirty/save-required
  unchanged single implementations).
- `Natural Voice pack · <state>` screen (feature builds only; a build
  without the capability shows nothing to install): status line
  (Not installed / Blocked—reason / Ready — synthesis verified),
  **Install voice pack** with the confirmation dialog (download size,
  added size, peak space, observed free space, reused espeak-ng, and
  the "does not change your main coding provider" note), real progress
  with Esc-stop, **Preview voice** (fixed phrase through the real
  adapter + PlaybackOwner; explicit action only; visible stop; never
  submits a turn, never opens the microphone, never commits the
  draft), **Repair / Verify** (asks before any new transfer),
  **Remove voice pack** (measured reclaim, ownership-safe), and
  **Details** (versions, revision, licenses Apache-2.0/MIT/GPL-3.0
  with espeak as an external system component, architecture, managed
  location).
- F9 gate: unchanged for the baseline; when the neural engine is
  saved, the SAME shared assessment adds the pack + phonemizer checks —
  enabled-but-blocked names the actual missing piece ("Voice is
  enabled, but …"), never "not enabled", never a silent fallback.

## 7. Red→green tests (new)

`apps/agent-vesper-tui/tests/r3_voice_pack.rs` (10 tests, real seams):
engine selection defaults to System; selection follows the saved scope
(incl. Heart default and non-kokoro ids staying System); scope save
round-trips engine+voice; pack readiness reports NotInstalled with the
install remedy; the gate separates Disabled from Blocked with neural
selected (real `save_voice_for_test` + real pack assessment; message
asserted); unverified record never reads Ready; removal reclaims
pack bytes and preserves neighbors; the defining clean-cache sequence
with deterministic assets (refusal-with-guidance before install →
mock synthesis through the real port → installation does not flip the
selection); setup readiness ≠ device acceptance labels; host
construction with an empty pack cache.
`crates/vesper-voice-kokoro` 35 unit tests: pinned manifest sizes/
URLs, budget headroom (const-evaluated), record/assess/verify
semantics, symlink refusal, remove ownership, identity churn, splitter/
normalization/possessive/initialism/number/currency/phoneme-post-
processing fixtures, ID wrapper contract `[0,…,0]` + context ceiling,
resampler determinism, style-row indexing convention, runtime-lib name
consistency, extraction rejection, digest exactness, cancel-before-work,
missing-pack refusal with guidance, empty/unknown-voice refusal,
truthful descriptor, empty catalog without a pack.
Red receipts from this unit (defects the tests/real-runs caught):
`.part`→`.part.done` staging mismatch (real run), runtime-library
validation against archive bytes (real run), q8f16 SIGSEGV (real run),
`while-let` stream non-termination in tests (hung run —
`Finished` is terminal, streams end by contract), lint/collapse fixes.

## 8. Real setup receipt (production installer path)

`cargo run -p vesper-voice-kokoro --features ort --example
real_setup_receipt` (isolated harness driving the production
`VoicePackSetup`; no manual seeding, no bypass):

- Plan: transfer 102 535 053 B; retained 118 004 068 B; peak
  120 802 497 B; available ≈ 283 GiB; revision `1939ad2a…`; runtime
  1.28.0; phonemizer present.
- **SETUP OK in 24.3 s** — stages Prepare/Vocab/Voices/Model/Runtime/
  Verify/Probe/Publish; the silent synthesis probe passed before
  publish.
- Post-install disk (measured): model 92 361 116; af_heart 522 240;
  am_michael 522 240; vocab 3 497; runtime lib 24 268 848; + license
  1 073 + notices 325 054 → **117 677 941 B measured** (manifest
  118 004 068 B counts the record + rounding of the two retained
  notice files; delta ≈ 326 KB = notices accounted exactly on disk).
  `du -sb` pack dir: 118 005 135 B (includes `pack.json`).
- Post-install assessment: **Ready — synthesis verified**.
- Budgets: retained 112.6 MiB < 256 MiB; peak 115.1 MiB < 512 MiB;
  ≥1 GiB free preserved (≈283 GiB observed before/after). No audition
  WAVs written; conversation response audio remains bounded-memory
  only (zero files); no voice-content logging added.

## 9. Performance receipt (release build, real inference, no device)

`cargo run --release -p vesper-voice-kokoro --features ort --example
perf_receipt`:

- Cold engine init: 319 ms (peak RSS at that point ≈ 151 MiB).
- Warm steady-state (after one warm-up pass per text):

| Voice | Text | Audio | Total synthesis | RTF |
|---|---|---|---|---|
| af_heart | "Understood." | 1.57 s | 0.995 s | 1.58× |
| af_heart | ordinary reply (2 sentences) | 7.42 s | 4.90 s | 1.52× |
| af_heart | technical (numbers/time/rev) | 15.35 s | 10.24 s | 1.50× |
| am_michael | "Understood." | 1.62 s | 1.03 s | 1.58× |
| am_michael | ordinary reply | 8.03 s | 5.26 s | 1.53× |
| am_michael | technical | 17.65 s | 11.86 s | 1.49× |

- **Audit correction:** the table's ratio is audio duration / synthesis
  time, not conventional RTF. Its values imply approximately 1.5× real-time
  throughput (conventional synthesis/audio RTF ≈ 0.63–0.67), not slower-than-
  real-time synthesis. Sentence buffering can still delay first playback.
  These historical measurements were not rerun in this repair and establish
  neither current audible latency nor Alex's acceptance.
- Peak process RSS: 815 MiB (includes the ~112 MiB model weights +
  ORT arena + voices). Post-cancel synthesis OK — engine not poisoned.
- Cancellation: pre-run cancel returns `Cancelled` (tested); mid-run
  cancellation completes the current ORT run then suppresses output
  (documented non-preemptible limit).

## 10. Gates (this machine, final tree)

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo xtask architecture` | 30 packages validated |
| `cargo xtask naming-guard` | clean (36 frozen hits) |
| `cargo xtask acceptance` | 23/23 |
| `cargo test --workspace --all-features` | 2 720 passed / 0 failed |
| `cargo test --workspace` (default) | 2 505 passed / 0 failed |
| `cargo build -p agent-vesper-tui --features voice-kokoro` | ok |
| deny.toml | no changes; new deps MIT/Apache-2.0 (ort, ndarray, half, smallvec, libloading) |
| Not run | MSRV 1.88 matrix, non-Linux targets (CI-gated); device tests (Alex-operated) |

Artifact: `target/debug/agent-vesper-tui` feature `voice-kokoro`,
sha256 `44e3d8da4d57fc9375dd16ff3b2879ef607a38a9dd439d76a6faf6e70e5a1cf6`,
350 979 840 B, built after the last source change; capability inclusion
byte-verified (`Install voice pack`, `Remove voice pack`, `Natural
Voice pack`, `model_quantized.onnx` strings present; default build
compiles none of it — checked). Installed `~/.local/share/agent-vesper`
binaries untouched; no release, tag, or publication.

Storage: `/home` avail 283 715 227 648 B at setup start → 276 508 712 960 B
at close (build growth in the existing `target/` + the pack; concurrent
desktop activity acknowledged). Voice runtime files created: the pack
only (`voice-pack/`, 118 005 135 B).

## 11. Limitations and open items

- **Alex's acceptance (open)**: listening quality, pronunciation
  acceptability, perceived latency, F9 flow with the pack, and live
  barge-in re-confirmation. Nothing here claims naturalness or
  device success.
- Historical performance ratios were mislabeled; see §9's audit correction.
  No latency acceptability claim is made without Alex's feedback.
- q8f16 remains unavailable on ORT 1.28.0 CPU (crash documented); a
  future runtime/model revision may revisit it — a new pinned decision,
  not a silent swap.
- espeak-ng IPA mispronunciations possible for unusual words (§4).
- Single supported target: x86_64 Linux (the runtime archive pin); the
  pack descriptor refuses elsewhere rather than guess.
- PR-5 (ACP host parity/release), default-build R20 migration, MSRV/
  cross-target CI: unchanged, out of scope.

## 12. How Alex uses it (verified paths)

```bash
cd /home/Alex/Projects/agent-vesper
cargo build -p agent-vesper-tui --features voice-kokoro
./target/debug/agent-vesper-tui
```

1. F3 (Settings) → **Voice** → *Natural Voice pack* → **Install voice
   pack** → review numbers → **Install** → watch real progress →
   "Ready — synthesis verified".
2. **Preview voice** to hear Heart on a fixed phrase (S stops it).
3. Back → **Speech engine** → Neural voice → pick Heart/Michael →
   **Back** → Esc → **Save changes**.
4. F9 conversation now speaks through the neural voice; barge-in/
   Ctrl+C behave exactly as in PR-4. Restart reuses the installed
   pack — no re-download.
5. **Remove voice pack** lives in the same screen; removal is
   confirmed, measured, and ownership-safe.

The pack is already installed on this machine by the real setup run
(§8), so step 1 will read "Ready" immediately.

## 13. Production-routing repair and audit

### Objective and status

2026-09-20. Repair Alex's repeated F9 silence and the model's substitution of
shell-generated robotic speech. Preserve text answers, selected Kokoro voices,
F5/shared STT, existing permissions and no-device verification. Baseline revision
`94ed16de502e98110498010b399b24659b17a63f` with extensive existing uncommitted VRO-17
work; no commit, reset, installation, download, release or unrelated cleanup.

**Verdicts:** source repairs and the local executable are verified on the scoped
fixtures below; real local Kokoro inference passes both voices through the real
binary; Alex's live provider/listening/F9/interruption acceptance remains OPEN.
The earlier “complete” claims and isolated-player assurances did not establish
this journey. This report does not claim all VRO-17 requirements are closed.

### Methods, findings and changed files

Read root/apps/TUI/test and docs/foundation contracts, inspected actual dispatch,
configuration, delta/terminal handling, worker and playback ownership. The user's
transcript proves a tool-authored flite/paplay detour, not a native Kokoro success.
No current trace proves every cause of the user's particular live silence.

- `src/voice_turn_instruction.rs` (new) and `src/main.rs`: F9 clones a turn-scoped
  host-delivery instruction into the real agent configuration. Direct/VRO inherit
  it; the ReAct request explicitly receives the same voice context. It says to
  answer normally, retain the answer in chat, leave speech to the saved host
  engine, and never discover/invoke another engine or audio player to satisfy
  “reply by voice”. Typed-turn configuration stays unchanged. This is a model
  instruction, **not** an arbitrary-shell security filter or a guarantee of model
  compliance. No new TTS tool, tool permission or reasoning engine was added.
- `src/main.rs`, `src/voice_conversation.rs`: terminal-only answers previously had
  no speech input. Feed their text through the same hygiene gate before runtime
  settlement; do not duplicate already-streamed answers. Split large text at
  UTF-8 boundaries instead of dropping a delta above 4096 bytes. Completion-only
  coverage is distinct from streaming coverage; mixed partial-stream/changed-final
  reconciliation is not established by these tests.
- `src/voice_speech_worker.rs`: queued jobs previously acquired their generation
  at dequeue, incorrectly making old work fresh after Stop. Stamp at enqueue and
  reject stale work before inference. Recheck after synthesis/frame retrieval.
  Admission is bounded to 32 queued/in-flight units, 8192 text bytes each, with
  visible rejection. Drop stops and requests shutdown. ORT work remains
  non-preemptible within a model call; stale output is suppressed, not replayed.
- `src/voice_playback.rs`: pipe writes and drain waits held the control lock,
  blocking Stop under backpressure. Release it during blocking IO/waits, retaining
  child identity when restoring stdin. Reset per-stream counters and bound busy-
  executable spawn retries. Worker write/start/end failures now propagate;
  Unknown completion is not Spoke, and pipe-write progress is no longer emitted
  as a playback acknowledgement. Only successful player drain produces Spoke;
  this is still not proof of human audibility.
- `src/main.rs`: speech failures persist as bounded metadata messages in visible
  chat instead of disappearing when ordinary completion overwrites status. While
  queued/in-flight, status identifies `Kokoro / <voice-id>` and synthesis/playback.
- `src/voice_conversation.rs`: speech Stop suppresses later deltas/terminal units
  for that reply without canceling the text answer; the next voice final clears
  suppression. A saved neural selection remains neural even in a non-Kokoro
  build, whose F9 gate refuses before capture rather than silently choosing system
  speech. Unknown non-neural provider identifiers are not newly audited here.
- `src/settings_host.rs`: Preview now uses the same speech worker and playback
  receipt/error handling. S/Esc invalidates/stops work and returns; failures are
  no longer discarded. Modal interaction/device acceptance and process-wide
  inference-session sharing between preview and conversation remain unverified.
- `src/lib.rs`, `Cargo.toml`: repair the `voice-conversation`-only worker gate,
  continuously drain system-engine outcomes, and declare required features for
  the neural-only repro examples and worker tests. Default-build compilation
  previously failed on those undeclared example/test dependencies. Move the
  existing library test module after production items for strict default Clippy;
  no dead-code waiver added.
- `tests/r3_loop_pty.py`: replace the old injected-AgentEvent shortcut entirely.
  The production `AGENT_VESPER_R3_LOOP` bypass is removed. The fixture serves the
  real LM Studio protocol over loopback, captures actual chat requests, and checks
  the F9 contract on the wire. Recorder/STT/player are doubles; selected Kokoro,
  phonemization, model, runtime, hygiene, worker and host dispatch are real.
  A phonemizer wrapper rejects eSpeak audio generation and permits quiet IPA only,
  so baseline substitution cannot pass. The balanced math case asserts a VRO
  route, not merely a selected mode. Both text and positive PCM are required.
- Ownership/evidence docs updated: TUI/tests AGENTS, foundation AGENTS, evidence
  index, this record, owning PRD. Root/apps/docs parent contracts and child indexes
  were reviewed and left unchanged: no new durable subtree or global workflow.
  ACP remains excluded because F9 and the playback owner are terminal-only;
  PR-5 remains out of scope. No foundational crate or adapter code changed here.

### Red and green evidence

First regression, with terminal-answer delivery absent (new seam initially a
no-op), ran:

```text
cargo test -p agent-vesper-tui --features voice-kokoro --lib terminal_only_answer_reaches_speech_once
completion-only answers must reach the same speech gate as deltas
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 268 filtered out
```

The generation test was also executed with the old dequeue-time generation
behavior reintroduced, then the fixed source restored:

```text
cargo test -p agent-vesper-tui --features voice-kokoro --lib stop_rejects_already_queued_generation
assertion failed: result_rx.try_iter().any(|result|
        result == SpeechOutcome::Stale { segment: 42 })
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 270 filtered out
```

These are scoped red regressions, not a claim that every added test was run on a
complete historical checkout. Other regressions cover no duplicate final speech,
Stop under pipe backpressure, new-turn recovery, selected-neural identity in a
minimal build and preservation/projection of the turn-only instruction.

Final scoped suites (all exit 0):

| Command | Verbatim result counts / scope |
|---|---|
| `cargo test -p agent-vesper-tui --features voice-kokoro` | `273 passed; 0 failed` lib; `155 passed; 0 failed` bin; integration groups 6/10/6/10 passed = **460 total** |
| `cargo test -p agent-vesper-tui --features voice-conversation` | 273 lib + 155 bin + 6/10/6 integrations = **450 passed**, no failures |
| `cargo test -p agent-vesper-tui` | `241 passed; 0 failed` lib; `154 passed; 0 failed` bin = **395 total**; unavailable voice tests feature-gated |
| `cargo clippy -p agent-vesper-tui --all-targets --all-features -- -D warnings` | `Finished dev profile` (exit 0) |
| `cargo clippy -p agent-vesper-tui --all-targets -- -D warnings` | `Finished dev profile` (exit 0) |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo xtask architecture` | `architecture boundaries validated for 30 packages` |
| `cargo xtask naming-guard` | `naming-guard: clean (36 hits, all frozen in baseline)` |
| `cargo xtask acceptance` | `Acceptance regression gate: 23 exact cases passed in 16799 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.` |

Preserved intermediate failures: default tests initially failed on feature-only
examples/worker imports; Clippy found a test initializer and pre-existing
items-after-test-module ordering. Both fixed and rerun. PTY harness initially
misclassified an optional embedding startup probe as a chat request, then searched
ANSI raw bytes for contiguous text; it now classifies the chat wire and asserts
the rendered screen. No production check was skipped to pass those failures.

Final real-binary receipts:

```text
python3 apps/agent-vesper-tui/tests/r3_loop_pty.py target/debug/agent-vesper-tui
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52000, 124800]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.

python3 apps/agent-vesper-tui/tests/r3_loop_pty.py target/debug/agent-vesper-tui /home/Alex/.local/share/agent-vesper/voice-venv/bin/python af_heart balanced
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro af_heart (balanced) -> fixture player; PCM bytes=[50400, 108000]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

Each fixture uses isolated HOME/config/session storage and a same-filesystem
hard-link cache for existing immutable assets; JSON metadata is copied, leases
are isolated, and temporary roots are removed on exit. Outbound proxies deny
non-loopback provider traffic. No downloaded model, duplicate large model,
response-audio file, real microphone or real speaker was used. Synthetic capture
files exist only within the temporary fixture and are removed. The fixture
returns fixed nonsensitive text and measures delivery bytes, not naturalness,
provider obedience, first-audible latency or live end-to-end performance.

### Built artifact and resource accounting

Final relevant source change preceded:

```text
cargo build -p agent-vesper-tui --features voice-kokoro
Finished dev profile [unoptimized + debuginfo]
700d080f36b6e950fbbecd4763155e3056a36f07c345ae0308ad420b112b544b  target/debug/agent-vesper-tui
351197448 bytes
```

Path: `/home/Alex/Projects/agent-vesper/target/debug/agent-vesper-tui`, Linux
x86_64, debug/dev, `voice-kokoro` (implies `voice-conversation`). Source revision
remains `94ed16de502e98110498010b399b24659b17a63f` plus dirty workspace changes;
this is not an immutable release identity. Capability is proven by real F9
execution above, not embedded strings or a version banner. Installed/running
binaries were not replaced.

Filesystem available bytes: **277204553728** at inspection → **277274615808**
after final tests/build. Concurrent filesystem activity prevents attributing the
net change to this repair. Existing `target/` reused, no cargo clean/new target or
release build. Exact Cargo-cache growth and peak RSS/setup usage were not newly
measured. No asset transfer/setup operation occurred; immutable model/runtime
inodes were reused rather than physically copied. The 1 GiB reserve was preserved
at observed checkpoints. This is not application-wide SSD-safety evidence.

### Limitations, deviations and readiness effect

- No claim that all installer/catalog/lease/preview/session-sharing/storage
  acceptance requirements were re-audited. Native setup/removal was not rerun;
  the original setup receipts remain historical. Full R20/default-build capture
  policy completion is not inferred from these tests.
- No live reasoning-provider call or model-obedience test, acoustic playback,
  naturalness, pronunciation, perceived latency, or repeated live F9 interruption
  acceptance. Alex's actual reported environment remains the final acceptance
  boundary. Instruction forbids model-authored speech detours but does not
  technically sandbox arbitrary shell commands.
- Current CPU benchmark/performance was not rerun. §9 corrects the prior inverted
  RTF interpretation; previous “3–4× faster release” and “near-instant” assurances
  have no evidence in this repair and must not be repeated.
- No full-workspace test matrix, ACP suite, CI/MSRV/cross-target or release gates
  rerun. Dependency/license manifests are unchanged by this repair; no new
  dependency or license-policy exception, and no new supply-chain claim.
- An attempted read-only background review was unavailable in this host; it
  contributes no independent-review evidence.

To run the repaired build, leave the previous TUI normally, then launch
`/home/Alex/Projects/agent-vesper/target/debug/agent-vesper-tui` (no rebuild or
install needed). Preserve the saved Kokoro/Heart or Michael selection. F9 starts
capture, F9 finishes capture; the host speaks its answer and keeps the text in
chat. During pending speech the status identifies the actual configured engine
and voice; F9 stops speech without substituting an engine. Errors stay in chat.
Preview remains an explicit Settings action. Alex's successful listening and
repeated-turn feedback is required before the live defect can be called closed.
