# Voice latency repair — measured local improvements, live latency acceptance open

## Objective and status

Alex confirmed Kokoro Preview and coding voice now work, but reported a multi-second wait before F9 recording and at least 20 seconds before replies. Remove avoidable delays without changing the selected voice, weakening pack integrity, pretending to record before the recorder starts, or disabling coding/reasoning features.

**Implemented and verified offline on Linux:** repeated whole-pack hashing is cached safely by file identity; duplicate readiness assessment is removed; enabled-pack preflight runs in the background at terminal startup; recorder start no longer waits for a separate Python import/probe; existing STT sidecar loading overlaps capture; Kokoro session preparation runs on the speech worker before the reply arrives. A fresh-data-root capture failure exposed by broader verification was also repaired.

**Not claimed:** instant end-to-end speech or a measured reduction of Alex's actual 20-second live-provider turn. Real transcription/provider latency was not measured in this work. Current CPU Kokoro inference remains nonincremental within each sentence and takes real time.

## Findings and methods

- `voice_readiness::neural_voice_checks` called `assess_pack` twice per gate; `pack::assess` rehashed all model/runtime/voice assets each time despite existing comments promising a cache.
- `voice::Worker::start` ran `prepare`, including `python -c 'import faster_whisper'`, before spawning the recorder. That serialized import work with microphone startup. The sidecar separately imports the module again.
- `SpeechWorker::build_engine` constructed a lazy Kokoro adapter, but the session was first loaded when the provider's first speech unit arrived.
- `SpeechOutcome::Progress` was not emitted after PCM writes, explaining the old launcher's misleading zero-byte progress counter.
- A read-only installed-pack benchmark measured assessment, session load and three fixed-phrase syntheses separately. It opens no audio devices, writes no audio and downloads nothing.
- Native F9 PTY tests use isolated HOME/workspace and loopback provider, fake recorder/STT/player, and real Kokoro. A deliberate 2-second fixture Python import verifies that recording no longer waits for it. The test allows startup integrity preflight to finish before timing F9. Player reads measure stop-to-first-PCM, not acoustic onset.

## Files and contracts

- `crates/vesper-voice-kokoro/src/pack.rs`: bounded cache of eight successful pack verifications, keyed by canonical root and full-pack metadata identity. On Unix, identity includes size/mtime plus device/inode/ctime; replacement with restored size/mtime invalidates. Changed/missing files rerun verification; failure is not cached; mutation detected across verification fails closed. `verify_pack` remains an unconditional full digest check. Non-Unix platforms retain unconditional hashing instead of a weaker metadata cache.
- `crates/vesper-voice-kokoro/src/adapter.rs`: public no-synthesis `prepare` for background session loading.
- `crates/vesper-voice-kokoro/examples/latency_receipt.rs`: explicit installed-pack, no-device timing helper.
- `apps/agent-vesper-tui/src/voice_readiness.rs`: one shared assessment per check.
- `apps/agent-vesper-tui/src/main.rs`: background read-only integrity preflight only for already-enabled neural voice; F9's fail-closed prerequisite gate remains.
- `apps/agent-vesper-tui/src/voice.rs`: known explicit/managed interpreter launches the sidecar directly instead of probing/importing before capture. Unknown interpreter setup remains on transcription's preparation path. No new automatic setup or model-selection behavior is introduced.
- `apps/agent-vesper-tui/src/voice_speech_worker.rs`: prepare the selected Kokoro adapter on its worker and emit actual accepted-byte progress. Stop/generation checks and text retention remain.
- `apps/agent-vesper-tui/src/voice_capture_store.rs`: measure the nearest existing ancestor when the data root has not yet been created; only NotFound permits ascent, while other failures still defer capture. Existing disk reserve, aggregate limit and ownership rules remain.
- `apps/agent-vesper-tui/tests/r3_loop_pty.py`: slow-import regression, recorder process onset timing, first-PCM timing and retained amplitude/provider-wire checks.
- Closest owning contracts, evidence index and voice PRD updated. No installed binary, saved voice, system volume, provider policy or user microphone/speaker was changed or used.

## Red → green evidence

### Repeated verification

The new instrumented cache regression failed before cache implementation:

```text
unchanged assets must not be rehashed
  left: 2
 right: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out
```

`cargo test -p vesper-voice-kokoro readiness_reuses` passed after implementation. The test also checks changed assets, same-size replacement with restored mtime, and missing components.

### Recorder startup

The repaired production recorder-start block was temporarily replaced with its exact previous prepare-before-capture block, rebuilt, and run against the new PTY regression. Other latency repairs remained in place to isolate this cause. The source was restored in a Python `finally` block and rebuilt before subsequent tests.

```text
Pre-repair recorder path test exit: 1
AssertionError: recorder start blocked: 2128.5 ms
```

Corrected final direct-route receipt:

```text
LATENCY: recorder process observed=251.0 ms; stop-to-first-PCM=1677.0 ms; complete fixture reply=4259.6 ms; includes PTY polling, synthetic STT/provider, real Kokoro; not real end-to-end latency
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52000, 124800]; peaks=[22327, 13045]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

The host's key helper polls for 250 ms before returning, so 251 ms is an observed upper-bound-style measurement at this resolution, not a precise device onset latency. The assertion is under 1000 ms with a deliberately 2000 ms import stall.

VRO receipt:

```text
LATENCY: recorder process observed=250.5 ms; stop-to-first-PCM=2244.8 ms; complete fixture reply=5764.3 ms; includes PTY polling, synthetic STT/provider, real Kokoro; not real end-to-end latency
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro af_heart (balanced) -> fixture player; PCM bytes=[50400, 108000]; peaks=[21248, 14919]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

### Fresh capture root

The broader `voice_pty.py` initially failed under the voice-capable build:

```text
Voice capture deferred: cannot determine free space for /tmp/vesper-voice-pty-vfirvgqu/data/agent-vesper
```

New unit regression before repair:

```text
fresh data root must be supported: UnknownFreeSpace { path: "/tmp/.tmpNdvrjD/data/agent-vesper" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 273 filtered out
```

After repair, all nine capture-store tests passed, including the new first-use case. The original broader fixture then passed without relaxing its checks:

```text
PASS: mouse/F5, 10-minute PCM, >90-second progressing transcription, editable composer, retry/discard, early exit, disk failure and shutdown cleanup
```

## No-device timing receipts

Command: `cargo run -p vesper-voice-kokoro --features ort --example latency_receipt`.

Before caching:

```text
assessment 0: 1576.929 ms
assessment 1: 1500.863 ms
assessment 2: 1500.707 ms
engine load: 1766.516 ms
synthesis 0: 3040.317 ms, 54400 samples
synthesis 1: 2206.712 ms, 54400 samples
synthesis 2: 2165.081 ms, 54400 samples
```

After caching, same two-thread engine and phrase:

```text
assessment 0: 1632.925 ms
assessment 1: 0.106 ms
assessment 2: 0.036 ms
engine load: 286.535 ms
synthesis 0: 2161.380 ms, 54400 samples
synthesis 1: 2161.493 ms, 54400 samples
synthesis 2: 2166.616 ms, 54400 samples
```

The cold verification cost is not erased; it moves off the normal F9 path via enabled-voice background preflight. A press before preflight finishes or after an asset change can still wait for full verification. This work does not claim a universal subsecond cold-start guarantee.

CPU thread experiment, rejected rather than advertised as an optimization:

- Four threads: synthesis 3156.206 / 3011.405 / 2993.384 ms.
- One thread: synthesis 2616.820 / 2606.201 / 2614.130 ms.
- Retained two threads (~2160 ms). These are local samples, not controlled cross-hardware benchmarks. No thread-count change remains.

## Final verification commands and receipts

- `cargo test -p vesper-voice-kokoro --all-features`:
  `test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`
- `cargo test -p agent-vesper-tui --features voice-kokoro --lib`:
  `test result: ok. 274 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s`
- `cargo test -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui voice::`:
  `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 148 filtered out; finished in 0.46s`
- `cargo test -p agent-vesper-tui --features voice-kokoro --test r3_speech_worker --test voice_playback_diagnostics`:
  `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s`
  and `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s`.
- `cargo clippy -p vesper-voice-kokoro -p agent-vesper-tui --all-targets --features agent-vesper-tui/voice-kokoro -- -D warnings`:
  ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.90s``.
- `cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui`:
  ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.62s``.
- `cargo xtask architecture`: `architecture boundaries validated for 30 packages`.
- `python3 apps/agent-vesper-tui/tests/r3_loop_pty.py`: direct receipt above.
- `python3 apps/agent-vesper-tui/tests/r3_loop_pty.py target/debug/agent-vesper-tui "$HOME/.local/share/agent-vesper/voice-venv/bin/python" af_heart balanced`: VRO receipt above.
- `python3 apps/agent-vesper-tui/tests/voice_pty.py target/debug/agent-vesper-tui "$HOME/.local/share/agent-vesper/voice-venv/bin/python"`: broader lifecycle receipt above.

Additional final checks:

- `cargo check -p agent-vesper-tui --bin agent-vesper-tui`: ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.51s`` (default build).
- `cargo test -p vesper-voice-kokoro`: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s` (bare crate).
- `git diff --check`: no output, successful. Report links from the evidence index and PRD resolved successfully.
- The workspace already contained extensive uncommitted voice work; aggregate Git diff statistics are not a measure of this repair's scope. Existing work was preserved.

## Final audit and unresolved acceptance

- Re-derived cache invalidation and recorder ordering invariants; red→green checks independently establish each repaired behavior. Frozen asset digests were not changed or bypassed. Cache entries are process-local and bounded.
- No live STT/model-provider/microphone/speaker latency was measured. The 20-second user report is preserved, not replaced by fixture timings. Actual latency after restart still needs user observation or separately authorized stage-timing measurement.
- First-time missing STT model/setup, cold/changed asset verification, runtime/model response time, sentence gating and CPU synthesis remain possible latency sources. The provider's coding/reasoning settings were not silently reduced to make the benchmark faster.
- Synthesis remains buffered per sentence; emitted progress proves accepted bytes, never audibility. Real-device acceptance from the preceding PCM repair is historical evidence, not a latency receipt for this change.
- Full workspace, release and cross-platform gates were not run. Non-Unix metadata caching is deliberately disabled. No installation/release was performed; already-running binaries need restarting against the rebuilt code.
- DOX pass updates nearest TUI/tests/Kokoro/foundation contracts and PRD/evidence links. Parent root/apps/crates/docs and child indexes remain unchanged because no ownership boundary or global workflow changed.

**Readiness effect:** materially lower verified local overhead and responsive recorder initiation with a known installed backend; instant/live end-to-end voice latency acceptance remains open.
