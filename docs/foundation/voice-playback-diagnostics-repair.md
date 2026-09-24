# Voice playback diagnostics repair — partial repair, acceptance open

> **Superseding audit:** Alex reported the original attempt was not heard. The
> [PCM scaling repair](voice-kokoro-pcm-scaling-repair.md) subsequently reproduced
> and fixed near-zero Kokoro samples; Alex confirmed one corrected playback clear
> and complete. The historical receipts below remain unchanged; broader native
> Settings/F9 acceptance is not implied.

## Objective and status

Investigate the shared Preview/F9 playback path without using microphone or speakers, preserve the selected engine and chat text, and stop hiding actionable player failures. **The diagnostic defect is repaired; the reported Kokoro silence is not resolved or accepted.** Successful shell speech with another engine is not Kokoro evidence.

## Methods and inspected files

- Read the root, apps, TUI, TUI tests, docs and foundation DOX contracts; inspected `src/voice_playback.rs`, `src/voice_speech_worker.rs`, `src/voice_conversation.rs`, `src/settings_host.rs` and `src/lib.rs` under `apps/agent-vesper-tui/`.
- Ran `aplay --help` only, not playback or device enumeration. Its output advertises `--fatal-errors`, raw input, S16_LE, sample rate and channel options used by the production command. This rules out an unsupported-option hypothesis on this installed player, not device failure.
- Added a device-free regression before changing production. The fixture drains PCM, emits a simulated connection refusal plus a private canary on stderr, then exits nonzero.
- Preserved the substantial pre-existing dirty workspace. No installer, user settings write, microphone, speaker, live provider, download or alternate TTS fallback was used.

## Changes and files

- `apps/agent-vesper-tui/src/voice_playback.rs`: drain stderr concurrently; retain at most its first 4096 bytes in memory for known-error classification. Only static messages may escape (connection refusal, permissions, busy/unavailable device or incompatible player); raw stderr is never displayed. Unrecognized failures retain exit status and an explicit unknown diagnostic. EOF collection waits at most 100 ms after child exit, outside the playback control lock. Pipe failures use the classification if already available.
- `apps/agent-vesper-tui/tests/voice_playback_diagnostics.rs`: connection-refusal/canary regression and 1 MiB stderr-pressure fixture.
- Owning TUI/test contracts document the diagnostic boundary; this report is linked from the foundation evidence index and voice PRD.

## Exact verification evidence

Before repair:

`cargo test -p agent-vesper-tui --features voice-conversation --test voice_playback_diagnostics`

```text
test device_failure_is_actionable_without_exposing_raw_stderr ... FAILED
playback failed: player exited exit status: 1
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

After repair:

`cargo test -p agent-vesper-tui --features voice-kokoro --test voice_playback_diagnostics`

```text
test device_failure_is_actionable_without_exposing_raw_stderr ... ok
test noisy_stderr_cannot_block_player_drain ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

`cargo test -p agent-vesper-tui --features voice-kokoro --test voice_playback_diagnostics --lib voice_playback`

```text
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 262 filtered out; finished in 0.10s
```

That filtered invocation ran zero integration tests; the separate unfiltered invocation above ran both. These unit tests include Stop under pipe backpressure, missing-player handling, bounded admission and nonzero-exit rejection. A test named `new_stream_after_stop_is_audible_again` uses a fake player and is **not audibility evidence**.

`cargo clippy -p agent-vesper-tui --features voice-kokoro --lib --test voice_playback_diagnostics -- -D warnings`

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.10s
```

The first Clippy run rejected a nested `if` (`clippy::collapsible_if`); the condition was collapsed and the command rerun successfully.

`cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui`

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.13s
```

The workspace debug binary was built; no installed binary was replaced. Temporary full command logs were written under `/tmp/vesper-playback-*.log`; the durable receipts are reproduced above.

## Deviations, unresolved items and readiness effect

- This is a diagnostic sub-repair, not completion of the voice repair. No evidence identifies the actual reason for the user's silent playback. A successful player exit cannot prove the selected output device is audible, unmuted or physically connected.
- Known diagnostics are heuristic matches of English player output, not authoritative device probes. Localized, later-than-4096-byte and unknown messages remain unclassified. An immediate pipe failure may precede diagnostic publication.
- No real-device Preview/F9 acceptance, real Kokoro inference, complete F5/F9 flow, cross-platform CI or release gates were rerun in this segment. Prior fixture receipts do not substitute for these missing checks.
- No routing, model instructions, STT, synthesis engine, cancellation policy, saved preferences or ACP behavior changed. This diagnostic projection is terminal playback-specific under the existing ACP exclusion.
- Next acceptance requires an explicitly user-operated Preview/F9 attempt with the newly built binary and observation of the actual outcome; real-device automation remains unauthorized.
- DOX pass: closest TUI/test contracts updated; root/apps/docs contracts intentionally unchanged because their ownership and workflow are unchanged. Foundation ownership/index updated; no child index boundary changed.

Readiness: safer actionable failure reporting, verified offline on Linux; overall Kokoro voice acceptance remains **OPEN**.
