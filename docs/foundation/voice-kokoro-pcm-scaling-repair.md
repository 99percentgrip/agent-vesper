# Kokoro PCM scaling repair and primary-source audit

## Objective, status and readiness effect

Resolve the reported silent Kokoro Preview/F9 output, check the integration against primary sources, and replace byte-count-only evidence with signal measurements and human listening acceptance.

**Confirmed defect repaired:** normalized float audio was rounded directly to integer PCM without multiplying by 32768. Both real installed voices consequently produced near-zero signal despite positive sample counts and successful player exits. After repair, real-model signal checks and direct/VRO F9 fixture paths pass. Alex explicitly authorized one corrected production-worker playback and confirmed **“Yes—clear and complete.”**

This closes the demonstrated amplitude-loss defect and one real-worker listening attempt. It does not constitute full Settings UI, live-microphone F9, pronunciation-quality, fresh-install, cross-platform or release acceptance.

## Primary-source research and integration comparison

Research was performed in this repair, after Alex challenged the earlier investigation. Earlier playback tests had not established amplitude correctness. The first `web_reader` attempt failed with `auth unavailable`; read-only Python `urllib.request` fetched the public sources instead. No packages or model assets were downloaded.

1. [Pinned ONNX model card](https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/raw/1939ad2a8e416c0acfeecc08a694d14ef25f2231/README.md), matching `pack::PINNED_REVISION`:

   ```python
   assert len(tokens) <= 510, len(tokens)
   ref_s = voices[len(tokens)]
   tokens = [[0, *tokens, 0]]
   # input_ids=tokens, style=ref_s, speed=np.ones(1, dtype=np.float32)
   wavfile.write('audio.wav', 24000, audio[0])
   ```

2. [Official ONNX JavaScript binding](https://raw.githubusercontent.com/hexgrad/kokoro/dfb907a02bba8152ca444717ca5d78747ccb4bec/kokoro.js/src/kokoro.js), revision `dfb907a02bba8152ca444717ca5d78747ccb4bec`, SHA-256 `97cbc36e4ab00fea35d3f8ca3a23ae199f787069f83774bb86e7886321c92e68`:

   ```javascript
   const SAMPLE_RATE = 24000;
   const num_tokens = Math.min(Math.max(input_ids.dims.at(-1) - 2, 0), 509);
   const { waveform } = await this.model(inputs);
   return new RawAudio(waveform.data, SAMPLE_RATE);
   ```

   Uses float32 style `[1, 256]` and speed `[1]`. Vesper's named tensors, padded int64 token input, style row selection and 24 kHz model rate match this export convention. The official Python `.pt` pipeline uses a different style indexing expression; it is not a reason to change the ONNX binary style row. Corrected Vesper's contradictory “1-based/num_tokens - 1” comment; implementation already matched the ONNX binding.

3. [SciPy WAV reference source](https://raw.githubusercontent.com/scipy/scipy/main/scipy/io/wavfile.py), fetched SHA-256 `6518765a973445f04a154c6dffac61b4f12a6c39d4fc78ff223a8452443ac187`, documents the representations used by the model-card example:

   ```text
   32-bit floating-point  -1.0         +1.0         float32
   16-bit PCM             -32768       +32767       int16
   ```

   Vesper must convert amplitude as well as sample rate before sending `S16_LE` to the player. It previously converted only rate. Correct conversion scales normalized amplitude, rounds and saturates, then serializes little-endian. It does not adjust system volume, normalize each utterance or substitute an engine.

4. [Official JavaScript phonemizer](https://raw.githubusercontent.com/hexgrad/kokoro/dfb907a02bba8152ca444717ca5d78747ccb4bec/kokoro.js/src/phonemize.js), SHA-256 `fc8f3c63edd56bdaa967b92274ad6df891a1926211fbc884675b7069f398a2d4`: inspected language selection, normalization and IPA post-processing. Vesper's two shipped voices are American; its fixed `en-us` phonemizer matches that scope. Its documented unusual-name/abbreviation and full-G2P parity limitations remain; this was not an exhaustive linguistic conformance evaluation. The attempted Transformers.js `main/src/utils/audio.js` URL returned HTTP 404 and supplies no evidence.

## Methods and files

Read applicable root/crates/Kokoro, apps/TUI/tests and documentation DOX chains. Inspected model input/output handling, voice style parsing, phonemization/token ceiling, adapter wiring and setup probe. Read-only `pactl`/`wpctl` and ALSA configuration inspection showed an unmuted default Speaker sink at 100%, with ALSA default configured for PipeWire. This did not prove audibility and no routing or volume change was made.

Changed:

- `crates/vesper-voice-kokoro/src/engine.rs`: normalized float → signed PCM amplitude scaling; corrected contract comments; independent endpoint/quiet-signal regression assertions; replaced the old already-integer-scale sine fixture and tautological i16 range assertion with a normalized sine and meaningful amplitude bound.
- `crates/vesper-voice-kokoro/examples/signal_receipt.rs`: explicit no-device real-installed-model probe for both supported voices; fixed nonsensitive Preview phrase, sample/nonzero/peak/RMS metrics, no audio files or player. Fixed-phrase peak/RMS thresholds catch near-zero conversion, not universal perceptual quality.
- `apps/agent-vesper-tui/tests/r3_loop_pty.py`: fake player records PCM peaks as well as byte counts; host acceptance rejects near-zero samples. Still isolated HOME/workspace, loopback provider, fake recorder/STT/player, read-only reuse of verified model assets.
- Owning Kokoro/TUI-test/foundation contracts, evidence index and voice PRD updated. Prior reports receive explicit superseding notes rather than silently rewriting their historical receipts.

No live microphone, live provider, alternate speech engine, installer, system-volume mutation or saved-settings mutation was used. No installed Vesper binary was replaced. The pre-existing dirty workspace was preserved.

## Red evidence: before production repair

`cargo test -p vesper-voice-kokoro --features ort normalized_`

```text
left: [-2, -1, -1, 0, 0, 0, 1, 1, 2]
right: [-32768, -32768, -16384, -8192, 0, 8192, 16384, 32767, 32767]
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 35 filtered out; finished in 0.00s
```

`cargo run -p vesper-voice-kokoro --features ort --example signal_receipt`

```text
voice=am_michael samples=54400 nonzero=28 peak=1 rms=0.023
voice=af_heart samples=48800 nonzero=2 peak=1 rms=0.006
canonical PCM signal is near-zero: float-to-s16 scale regression
```

Exit 101. Positive byte/sample counts had hidden essentially silent PCM. Alex's listening result on the previous uncorrected probe was explicitly **“no i didn't hear”**, superseding that probe's previously unanswered interview.

## Green evidence: after repair

Same real-model signal command and phrase, no speakers:

```text
voice=am_michael samples=54400 nonzero=47183 peak=19765 rms=1885.037
voice=af_heart samples=48800 nonzero=40285 peak=26163 rms=2108.325
```

`cargo test -p vesper-voice-kokoro --features ort,mock-synthesis`

```text
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo test -p vesper-voice-kokoro`

```text
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo clippy -p vesper-voice-kokoro --all-targets --all-features -- -D warnings`

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.19s
```

`cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui`

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.62s
```

`cargo xtask architecture`

```text
architecture boundaries validated for 30 packages
```

`python3 apps/agent-vesper-tui/tests/r3_loop_pty.py`

```text
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52000, 124800]; peaks=[22327, 13045]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

`python3 apps/agent-vesper-tui/tests/r3_loop_pty.py target/debug/agent-vesper-tui "$HOME/.local/share/agent-vesper/voice-venv/bin/python" af_heart balanced`

```text
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro af_heart (balanced) -> fixture player; PCM bytes=[50400, 108000]; peaks=[21248, 14919]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

`cargo test -p agent-vesper-tui --features voice-kokoro --test voice_playback_diagnostics`

```text
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

## Authorized listening evidence

After warning that corrected amplitude is materially louder and the output was at 100%, the user selected:

```text
corrected_preview: Yes—play one corrected Kokoro sample now
```

Exactly one `cargo run -p agent-vesper-tui --features voice-kokoro --example r3_worker_device_check`:

```text
engine: Ready
player: /usr/bin/aplay
[t+9.350757806s] SPOKE: 71200 samples (0 bytes total)
worker device check OK
```

The existing launcher's zero-byte counter is not authoritative; see the [prior audit note](voice-kokoro-authorized-playback.md#audit-note-misleading-launcher-labels). It was not changed in this repair. Human feedback, not its `SPOKE` label, supplies listening evidence:

```text
corrected_heard: Yes—clear and complete
```

This used the saved `am_michael` selection and the real production speech worker/player. It was not an automated Settings modal test or a live-microphone F9 session.

## Final audit, deviations and remaining acceptance

- Re-derived normalized float → signed 16-bit endpoint and quiet-signal invariants independently of the implementation. New unit assertions and real-model signal probe failed before the fix, then passed after it. Host fixture amplitude checks prevent positive-length silent PCM from satisfying this gate again.
- Corrected stale style-index/sample-output comments, the misleading test amplitude and the earlier unresolved-listening narrative with explicit superseding evidence. No model-weight/digest, provider, runtime, fallback or saved-voice change was needed.
- The shared Kokoro adapter conversion serves any consumer; native F9/player presentation remains terminal-specific under the documented ACP exclusion. No separate ACP playback path exists to patch.
- Full workspace tests, complete TUI suite, fresh installation, all-target release CI and complete real Settings/F9/microphone acceptance were not run. No claim that all voice implementation issues are eliminated is supported by this work.
- The current developer build contains the repair; an already-running or separately installed old binary does not acquire it automatically.
- DOX ownership updates are local. Root, parent crates/apps/docs and TUI main contract remain unchanged because boundaries and global workflows did not change; no Child DOX Index changed.

**Delivery status:** confirmed PCM-scaling defect repaired with offline red→green, actual host signal evidence and one successful user-confirmed listening attempt. Wider voice acceptance remains explicitly scoped above.
