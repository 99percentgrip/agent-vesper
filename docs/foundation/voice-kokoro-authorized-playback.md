# Authorized Kokoro playback probe — original listening attempt failed

> **Superseding audit:** Alex reported the original attempt was not heard. The
> [PCM scaling repair](voice-kokoro-pcm-scaling-repair.md) subsequently reproduced
> and fixed near-zero Kokoro samples; Alex confirmed one corrected playback clear
> and complete. The historical receipts below remain unchanged; broader native
> Settings/F9 acceptance is not implied.

## Objective and status

Continue the [playback diagnostic investigation](voice-playback-diagnostics-repair.md) with one explicitly authorized short Kokoro playback, without microphone access. **The player reported a successful drain, but Alex confirmed he did not hear the original attempt. Complete Preview/F9 acceptance was not established.** This is not voice-feature completion.

## Authorization and methods

The planning interview returned this exact choice:

```text
audio_acceptance: Run one short Kokoro Preview through the production playback path; no microphone
```

Read `apps/agent-vesper-tui/examples/AGENTS.md` and the existing `r3_worker_device_check.rs` launcher before execution. Read only the workspace voice table's enabled/engine/voice values:

```text
{'enabled': True, 'tts': 'voice-kokoro', 'voice': 'am_michael'}
```

The existing launcher uses `am_michael`, the real installed pack, the production `SpeechWorker`, and `PlaybackOwner` with the PATH-resolved `aplay`. Its fixed sentence is “Understood. The voice worker is speaking through your speakers now.” The evaluation bypasses the Settings UI, microphone and agent turn. It is a shared-production-path probe, not a native Settings Preview interaction test. The launcher separately checks engine readiness before constructing the worker.

Exactly one playback invocation:

```sh
cargo run -p agent-vesper-tui --features voice-kokoro --example r3_worker_device_check
```

No fallback engine, microphone, volume change, saved-settings change, installer, live provider or repeat playback was used.

## Exact evidence

```text
   Compiling agent-vesper-tui v0.23.3 (/home/Alex/Projects/agent-vesper/apps/agent-vesper-tui)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.50s
     Running `target/debug/examples/r3_worker_device_check`
worker device check: real pack + real aplay (audio will play)
engine: Ready
player: /usr/bin/aplay
[t+9.33885453s] SPOKE: 71200 samples (0 bytes total)
worker device check OK
```

Command returned successfully. `voice_speech_worker.rs::speak_one` accepts each audio frame only after `PlaybackReceipt::BytesWritten` and emits `SpeechOutcome::Spoke` only after `PlaybackReceipt::Drained`; the owner emits that receipt after a successful player exit following stdin EOF. This establishes the software path's successful handoff and drain, not physical audibility.

### Audit note: misleading launcher labels

The printed `0 bytes total` is **not a valid byte measurement**. The launcher increments its local `progress_bytes` only on `SpeechOutcome::Progress`, but the inspected worker audio loop does not emit that progress event while accepting frames. It counts samples separately. The 71,200 reported samples represent 4.45 seconds at the canonical 16 kHz rate; no acoustic duration was measured. The launcher's `SPOKE` and `OK` labels must not be interpreted as a human-heard result. The launcher was inspected and executed as pre-existing work, not modified in this segment.

## Listening confirmation and unresolved items

A second interview asked whether the user heard the exact sentence, with clear/distorted/no-sound choices. It returned:

```text
tool error: tool execution failed: request_human_input failed: timed out waiting for human review
```

**Audit correction:** the interview initially timed out, but Alex subsequently answered “no i didn't hear” twice. The original listening attempt therefore failed; its status is not unknown. No automatic repeat was authorized by that negative result.

- The subsequent [PCM scaling repair](voice-kokoro-pcm-scaling-repair.md) identified the near-zero signal defect and records a separately authorized corrected playback and positive listening confirmation.
- That later result does not retroactively make this original attempt audible.
- Native Settings Preview interaction and full F9 capture → transcription → model response → selected speech plus chat retention were not executed here.
- No cross-platform or release readiness claim is made.
- The original no-device constraint was relaxed only for the single explicitly approved output attempt; microphone use remains unauthorized.

## Files, verification and DOX

This report, foundation ownership/index and voice PRD links are the only changes in this continuation. No source changes or new program tests were needed. `git diff --check` passed before reporting; final link/whitespace review follows the prose-only verification rule. Existing offline red→green tests and scoped Clippy/build receipts remain in the preceding diagnostic report; they are not substituted for missing listening acceptance.

Nearest foundation ownership is updated. Root/apps/TUI/tests/examples contracts are intentionally unchanged: this probe adds evidence, not a durable runtime behavior, workflow or ownership boundary. No child index changed.

## Readiness effect

The original Kokoro-to-player software path drained one authorized attempt, but the human listening result was **negative**. The later PCM repair supersedes the unresolved-cause status; this historical probe alone closes no native Preview/F9 acceptance.
