# PRD: Voice trailing-silence hallucination guard (VAD)

| | |
|---|---|
| **Status** | COMPLETE — 2026-09-14, shipped in the same session (small, single-surface change) |
| **Target** | `apps/agent-vesper-tui/src/voice_transcribe.py` (+ test harnesses) |
| **Owner** | Alex (product); implementation via fast-track directive |
| **Related** | `voice-control-prd.md` (§scope: "does not promise perfect speech recognition"), `voice-f5-cancel-trap-repair.md` (field report that surfaced this) |

## 1. Problem (observed on Alex's real device, v0.22.8)

A >60 s dictation transcribed correctly but ended with ~77 repetitions of
"1 min". Mechanism: faster-whisper hallucinates text on trailing
silence/room noise. Reproduced on the real installed model
(`~/.local/share/agent-vesper/voice-venv`, `tiny`, int8, CPU):

- 30 s of pure zeros, **no filter** → `"You"`
- 30 s of pure zeros, `vad_filter=True` → `""`

The dictation pipeline records until Stop, so trailing silence is the
normal case, not an edge case. Root cause: `voice_transcribe.py` called
`model.transcribe(samples)` with **no VAD**.

## 2. Decision

**D1 — enable faster-whisper's built-in VAD.** Pass `vad_filter=True` on
every production `transcribe` call (default VAD parameters; no custom
tuning without evidence). Rejected alternatives: post-trimming the audio
ourselves (duplicates a solved problem, more code to maintain, energy
thresholds to tune); hallucination post-filtering by log-prob (model-
specific, fragile). `vad_filter` is the library's own answer to exactly
this defect and is supported by the pinned faster-whisper API (checked
against the installed venv via signature inspection:
`vad_filter`, `vad_parameters`, `hallucination_silence_threshold`).

**D2 — verify by contract, not by hope.** The sidecar protocol is
exercised by two Python harnesses with stand-in models; both now assert
`vad_filter=True` arrives at the model on every call (fixture records
filtered calls to a `vad` file; the plain-model harness asserts the kwarg
directly). The PTY suite additionally counts ≥2 filtered calls (resume +
retry paths). Real-model behavior is pinned by the probe receipt below,
not by a test (no model download in foundation tests, per contract).

## 3. Acceptance criteria

- AC-1: Red-first — new PTY assertion fails against the pre-fix sidecar.
- AC-2: Green — PTY suite passes with the fixed sidecar (all lifecycle
  paths unchanged otherwise).
- AC-3: `voice_chunks.py` (production slicing, real numpy) passes and
  asserts the kwarg on every call.
- AC-4: Real-model probe through the production script: VAD reaches the
  model; near-silence transcribes to `""`.
- AC-5: No Rust behavior change: workspace/acceptance/clippy/fmt/
  naming-guard clean.

## 4. Results

All five met. Receipts in `voice-vad-silence-guard-execution.md`.
