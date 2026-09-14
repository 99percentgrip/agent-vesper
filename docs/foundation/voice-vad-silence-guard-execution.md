# Voice VAD silence-guard execution

Date: 2026-09-14. Status: **complete**.
Owner PRD: [Voice trailing-silence VAD](../voice-trailing-silence-vad-prd.md).

## Changes

| File | Change |
|---|---|
| `apps/agent-vesper-tui/src/voice_transcribe.py` | `model.transcribe(samples, vad_filter=True)` + comment citing the measured probe |
| `apps/agent-vesper-tui/tests/voice_model_fixture.py` | `transcribe(**kwargs)`; appends a `vad` marker per filtered call |
| `apps/agent-vesper-tui/tests/voice_chunks.py` | plain-model harness asserts `vad_filter=True` kwarg on every call |
| `apps/agent-vesper-tui/tests/voice_pty.py` | resets + asserts the `vad` marker file (≥2 filtered calls across resume paths) |

No Rust changes; the sidecar script is `include_str!`-compiled into the
TUI binary (a lesson re-learned below).

## Red-first receipts (AC-1)

New assertion run against the **pre-fix** sidecar (test-side changes
re-applied onto the stashed tree, production script left old):

```
AssertionError: production sidecar must pass vad_filter=True
exit: 1
```

(Process note: the first "red" attempt stashed fix + tests together and
proved nothing — a trivially green old suite. The second attempt isolated
the production script correctly. A later full-suite "failure" was also
self-inflicted: the PTY suite ran against a binary built before the fix;
the script is baked into the binary at compile time. Rebuilt → green.)

## Green + behavioral receipts (AC-2..4)

- PTY suite through the production binary, fixed sidecar: **PASS**,
  full lifecycle unchanged (`/tmp/vad-green2.log`).
- `voice_chunks.py`: **PASS** — ten-minute PCM, bounded slices, ordering,
  partial + completed retry, kwarg asserted per call.
- Real-model probe (installed venv, `tiny` model, production script
  loaded directly, spy on `model.transcribe`): `vad_filter seen by model:
  [True]`; 25 s of synthetic room noise → `{"index": 0, "text": "",
  "seconds": 30}` — silence now yields empty text instead of
  hallucinated words.
- Earlier primary-source probe (the PRD's basis): 30 s zeros → `"You"`
  unfiltered vs `""` filtered, on the same installed model.

## Gates (AC-5)

Workspace **2,326 / 0 failed** · acceptance **23/23** · naming-guard 18
frozen · fmt clean. No Rust source touched.

## Deviations / open items

- Default VAD parameters only; tuning (e.g. `speech_pad_ms`) reserved
  until there's evidence of clipped first words in the field.
- Alex's device is the real acceptance surface again — next long dictation
  should no longer end in repeated hallucinated tokens.
- Rust-side tests unchanged because the guard lives entirely in the
  Python protocol; the Rust worker treats transcription text opaquely.

## Readiness effect

Dictations ending in silence no longer append hallucinated repetitions
(the "1 min" ×77 artifact): trailing quiet is filtered before the model
sees it.
