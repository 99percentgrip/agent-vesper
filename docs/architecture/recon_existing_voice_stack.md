# Existing Voice Stack Reconnaissance (before PR-2)

Read-only reconnaissance requested by Alex after reporting that Vesper
already has push-to-talk: establish exactly what the current
implementation provides before selecting another speech engine or
building overlapping functionality. **No production code, tests,
manifests, settings, dependencies, or assets were modified.** This
report and its index entries are the only workspace changes.

Mission rule honored: push-to-talk's existence was treated as proving
nothing about speech output; both directions were traced independently.

## Evidence pinning

| Item | Value |
|---|---|
| Workspace revision | `94ed16de502e98110498010b399b24659b63f` → `94ed16d` on `main` (2026-09-18 01:00:52 +0800, v0.23.3 release commit) |
| Dirty state | PR-0/PR-1 VRO-17 work uncommitted (crate + docs + guard edits); no other changes; preserved untouched |
| Installed binaries | `~/.local/share/agent-vesper/{agent-vesper-tui,agent-vesper-acp}` 0.23.3, built 2026-09-18 01:52 |
| Running processes | two `agent-vesper-tui` instances (the installed 0.23.3 binary) |
| Binary ↔ workspace | **Corresponds.** Installed-binary strings match this workspace's dictation source byte-for-byte (`arecord -q -f S16_LE -r 16000 -c 1 -t wav`, the embedded `voice_transcribe.py` text, exact status strings). The uncommitted PR-0/PR-1 `vesper-voice` crate is in **no** binary — Alex's push-to-talk experience is the shipped v0.23.3 dictation path traced below, not VRO-17 work |

Scope of the "not found" claims below: all Rust sources under `apps/`
and `crates/`, both app manifests and feature tables, the workspace
`Cargo.toml` dependency set, the full `Cargo.lock` (audio/speech crate
scan), TUI settings/keybinds menus, ACP sources, shipped scripts, seed
skills, docs, the installed voice venv's package list, and locally
installed executables. Not inspected: unrelated user dotfiles, other
machines, anything outside this checkout and its standard install roots.

## 1. Capability inventory

| Capability | Status | Classification |
|---|---|---|
| Microphone capture (push-to-talk) | **Implemented, shipped, in the running build** | Production-reachable |
| STT transcription (local Whisper) | **Implemented, shipped** | Production-reachable |
| Transcript → composer insertion | **Implemented, shipped** | Production-reachable (edit-before-send) |
| Auto-submit of transcript as agent turn | **Not present** (by design) | Documented intent |
| Assistant speech synthesis | **Not found in the inspected scope** | Gap |
| Speech playback | **Not found in the inspected scope** (terminal bell only) | Gap |
| Sentence-by-sentence dispatch | **Not found** (PR-3, not started) | Gap |
| TTS hygiene/redaction | **Not found** (PR-0 contract only) | Gap |
| Barge-in/interruption (voice side) | **Not found** (runtime turn-cancel exists; voice wiring is PR-3) | Gap |
| PR-0 contracts + PR-1 STT adapters | Implemented in workspace, **unreferenced by any host** | Optional code not enabled in any build |
| `espeak-ng`, `flite`, `spd-say` on this machine | Binaries present; **no Vesper code invokes them** | Installed dependency without integration |
| TTS library in the voice venv | None (venv holds `faster_whisper`, `ctranslate2`, `onnxruntime`, `av`, … — no TTS package) | — |
| Neural local TTS (piper et al.) | Not installed, not integrated | Docs-only candidate (PRD §2.7) |

## 2. Actual call paths

### 2.1 Push-to-talk (microphone → text → composer)

Traced end-to-end in current source (all paths verified this session):

1. **Activation**: footer chip action `toggle_voice` bound to `F5`
   (`main.rs:4306`, `:15716`; keybinds file has no voice override —
   `~/.config/agent-vesper/keybinds.json` doesn't exist, so the default
   binding is live). Footer chip renders `● Push to talk` → `■ Stop`
   (`ui.rs:407-421`), reserved before lower-priority footer actions,
   F5 stays reachable through permission/menu interceptors
   (`main.rs:1791-1805`).
2. **Toggle** → `toggle_voice_recording` (`main.rs:4558`) →
   `voice::Controller::toggle` (`voice.rs:79-117`): a dedicated worker
   **thread** owns all audio and subprocesses (`voice.rs:1`); UI thread
   only reads `Snapshot{phase, elapsed, detail}` — truthful recorder
   state projected, never inferred from text.
3. **Capture**: `arecord -q -f S16_LE -r 16000 -c 1 -t wav <tmpfile>`
   (Linux) / `afrecord -f WAVE -d LEI16@16000 -c 1` (macOS) into a
   `tempfile::TempDir` (`voice.rs:360-379`); recorder spawned in its own
   process group with RAII `Drop` kill+wait (`voice.rs:126-152`); Windows
   unsupported, honestly (`voice.rs:346-348`).
4. **Backend bootstrap** (first use only, worker-side, cancellable):
   probe candidate Pythons → harness venv at
   `$AGENT_VESPER_VOICE_VENV`/XDG/`~/.local/share/agent-vesper/voice-venv`
   via bundled `uv` (fallback `python3 -m venv` + pip), install
   `faster-whisper`, verify import (`voice.rs:300-355`, `main.rs:4444-4470`).
   No unapproved downloads at runtime beyond that documented setup.
5. **Transcription**: warm sidecar `python -c <voice_transcribe.py>`
   with stdin JSON protocol; 30-s PCM chunks; **`vad_filter=True` on
   every `transcribe` call** (the binding hallucination guard,
   `voice_transcribe.py:38-41`); ordered chunk assembly, 5-min stall
   bound, retry/discard with private audio retention (`voice.rs:381-511`).
6. **Delivery**: `take_text()` drained each frame by `drain_voice`
   (`main.rs:1477`, `:4562-4570`) → **appended to `session.input` with
   cursor placement; the turn is NOT auto-submitted**. The user edits
   and presses Enter like any composer text. On success the audio file
   is deleted ("Audio deleted; review before sending").
7. **Lifecycle guards**: Settings blocked while voice active
   (`main.rs:1346-1350`); Del cancels Preparing/Transcribing or discards
   a failed clip (`main.rs:1795-1805`); error strings carry the
   Retry/Discard contract.

Local vs remote: **entirely on-device** (arecord → local Python → local
model). Streaming vs buffered: capture is streamed to a WAV file;
transcription is chunked-buffered (30 s windows) with incremental
progress; no partial transcripts exist (final only). Cancellation:
worker `AtomicBool` + process-group kill; cleanup is RAII-owned.
Tests/receipts: PTY suites (`voice_pty.py`, `voice_chunks.py`,
`voice_model_fixture.py`, `voice_recorder_fixture.py`) and the
real-device/VAD evidence recorded in `docs/voice-control-prd.md`,
`docs/voice-trailing-silence-vad-prd.md`, and the v0.22.6 release
execution. (Historical receipts cited, not re-run; two targeted no-device
suites re-executed this session — §4.)

### 2.2 Speech output (assistant text → synthesis → playback)

**Not found in the inspected scope.** The trace, so the negative claim
is auditable:

- The only audio emission in the TUI is the **terminal bell** `\x07`
  written on agent-turn completion when the `sound` preference is on
  (`main.rs:8160-8164`). That is a notification beep — not synthesis,
  not speech.
- No TTS/synthesis module, read-aloud command, or speak helper exists in
  `apps/agent-vesper-tui/src/*` (29 modules inspected by name and by
  `tts|speak|speech|synthes|read.?aloud|espeak|piper` grep) or in ACP
  (zero voice surface, including its feature table).
- No audio-output crate in the dependency graph: `Cargo.lock` contains
  none of `cpal`, `rodio`, `libpulse*`, `alsa*`, `portaudio`, `libao`,
  `speech-dispatcher`, `tts`, `oespeak`, `piper*`.
- No Rust source spawns any speech binary; the locally installed
  `espeak-ng`/`flite`/`spd-say` are invoked by nothing in this repo.
- The installed voice venv contains **no TTS package**.
- Docs promise no speech output anywhere I checked (voice-control PRD,
  using-vesper, installation, README).
- PR-0's `VoiceTts` port + fakes are the only TTS-shaped code in the
  workspace; they are ports and fixtures, unreferenced by any host —
  per PRD and ADR 0028, not a capability.

So: Alex's push-to-talk is real and shipped; **assistant speech is not
implemented and not reachable in any build, installed or workspace.**

## 3. Reuse-versus-gap matrix (existing stack vs PR-0/PR-1)

| Component | Recommendation | Smallest seam / evidence |
|---|---|---|
| `arecord`/`afrecord` capture + RAII process guard | **Reuse unchanged** for dictation; wrap behind PR-0 `PcmFrame`/budget when voice-conversation mode lands (PR-4) | The recorder already emits canonical 16 kHz mono i16 WAV — byte-identical to the PR-0 canonical contract (`audio.rs`). No second capture owner should be created |
| Shipped sidecar script + venv bootstrap + cached models | **Already reused** by PR-1's `stt-sidecar` via `include_str!` of the same file (single source of truth); keep as the one STT inference path | `stt_sidecar.rs:38-40` embeds `apps/agent-vesper-tui/src/voice_transcribe.py`; same venv/models, no duplicate downloads |
| `voice.rs` worker/thread model | **Wrap, don't replace**: it remains the dictation owner; PR-4's conversation mode should drive the PR-1 adapter from a similar host worker | Two sidecar children can coexist today only because PR-1 is host-unwired; when wired, share **one** adapter instance between dictation and conversation mode to avoid duplicate model memory |
| VAD binding (`vad_filter=True`) | **Reuse unchanged** — preserved in script, adapter, and PR-0 descriptor rule | VAD PRD probe + PR-1 real-model receipts |
| PR-0 ports/errors/egress/budgets/report/events | **Reuse unchanged** | `crates/vesper-voice/src/*` |
| PR-1 `FailoverStt`/`PartialGate`/`run_blocking` | **Reuse unchanged** | `composition/*`; partial gate is exactly the mechanism the existing dictation lacks |
| Terminal bell on completion | Keep as the notification; **not** a speech substitute | `main.rs:8160` |
| Local `espeak-ng`/`flite`/`spd-say` binaries | **Record, do not adopt yet** — candidate subprocess backends (below), Alex decides | Present at `/usr/bin/{espeak-ng,flite,spd-say}`; zero call sites |

Duplication check (asked explicitly): model initialization, sidecar
processes, capture ownership, provider configuration, and cancellation
paths — **no live duplication exists in any build** because PR-1 is not
host-wired; the one real future risk is two sidecar instances
(dictation worker + conversation adapter) once PR-4 wires both, noted
above. Existing working synthesis to assess before replacing: **none
exists**, so no replacement question arises.

### The actual missing work, separated (not "TTS missing," one bucket)

1. **Synthesis itself** — no engine integration of any kind. This is the
   only genuinely new *engine* question.
2. **Playback** — no audio-out path in the TUI (bell aside). For a
   subprocess backend that writes to the default output device
   (`espeak-ng`, `spd-say` do this natively), playback collapses into
   the synthesis call; for engines emitting files/streams a playback
   owner is additionally needed.
3. **Sentence-by-sentence dispatch** — PR-3's sentence gate +
   `VoiceSession` overlap model; unstarted by design.
4. **Text hygiene** — PR-2's hygiene/redaction engine; PR-0 froze the
   contract.
5. **Interruption handling** — runtime turn cancellation exists and is
   transactional (ADR 0007); voice-side barge-in wiring is PR-3.
6. **Connection to the agent response** — input side exists (composer);
   the output side (assistant stream → gate → TTS → playback, with acks)
   is PR-3/PR-4 and is absent.

## 4. Executed checks (separate from inspection)

- `cargo test -p agent-vesper-tui voice` → 1+7 passed, 0 failed
  (dictation unit paths, no device).
- `cargo test -p vesper-voice` → 59+13 passed, 0 failed (PR-0 contracts
  still green on this revision).
- Binary-identity check via `strings` on the installed TUI (matches
  workspace dictation source; contains no `vesper-voice` symbols).
- No full workspace suite was run (none needed for this report); no
  microphone opened; no playback started; no remote service contacted;
  no model or package downloaded.

## 5. Minimal proposed PR-2 scope correction (recommendation only —
not authorization)

PR-2's *contracted* scope (TTS adapters, hygiene engine, egress
enforcement, Alex's local-TTS option decision) remains valid; the
correction is about the **first adapter's shape**, informed by what
already exists:

- **Smallest next implementation task**: a `tts-subprocess` adapter in
  the PR-1 pattern (feature-gated, zero Rust dependencies) over a
  **user-installed system speech binary** — `espeak-ng` or `spd-say`
  are already on this machine; both synthesize *and* play in one
  invocation, which collapses gap #2 into gap #1 for the local path and
  needs no audio-out crate. `VoiceTts` streams per-sentence units, so
  sentence latency is formant-synth-fast. License note, recorded
  honestly: invoking a user-installed binary at a process boundary is
  not distribution of that binary — the PRD §2.7 option-(b) shape — but
  the engines' own licenses differ (`espeak-ng` GPL-3.0-family;
  `flite` historically BSD-style — **verify against primary sources
  before any adoption**; not verified here, no approval implied).
  Voice quality is robotic versus neural TTS; that is the honest
  trade-off for the smallest seam.
- **Does it genuinely require another engine?** Synthesis does not
  exist, so *some* engine is required — but **not a new Rust dependency
  and not a download**: the smallest path reuses binaries already on
  the machine through the established subprocess pattern (the `arecord`
  precedent). A neural-quality local engine (GPL piper line) or a cloud
  adapter remains Alex's separate §2.7/R3 decision, unchanged.
- Everything else in PR-2 (hygiene engine + fixtures, egress
  enforcement) is dependency-independent and can proceed regardless of
  the engine choice.
- Regression baseline to preserve: the entire §2.1 dictation flow and
  its PTY/real-device receipts, byte-for-byte behavior (R10).

PR-2 has **not** been started; nothing was paused mid-flight — the
previous unit ended at the PR-1 gate, and that work is preserved
uncommitted as found.

## Verification

- Every workspace path cited exists (checked this session); line
  anchors refer to the current working tree at `94ed16d` + uncommitted
  VRO-17 files.
- Negative claims carry their scope (§ "Evidence pinning"); they are
  "not found in the inspected scope," not impossibility claims.
- No repository verification gates were run beyond the two targeted
  suites in §4 (docs-only change otherwise).
