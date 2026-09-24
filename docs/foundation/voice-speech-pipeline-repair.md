# Voice sentence-pipeline repair — listening confirmed; initial reply latency open

> **Follow-up:** [First-speech repair and NPU assessment](voice-first-speech-and-npu-assessment.md)
> adds an explicitly authorized live-turn measurement, long-unit subdivision and
> the actual-frame wait-status fix. Its latest contracts/evidence supersede this
> earlier sentence-only milestone; native NPU STT/TTS remain open.

## Objective, status and readiness effect

Remove the avoidable pauses between spoken sentences, inspect current primary documentation, and measure the PCM-fixed production path. The user authorized several short playback checks and requested elimination of high latency.

**Bounded repair complete on Linux:** the speech worker prepares one successor while the preceding unit plays. The device-free regression failed on the serial implementation and passes on the pipeline. Alex confirmed the serial check had a delay and the overlapping check did not. This is evidence for the sentence-gap repair, **not completion of the broader initial-reply latency requirement**. The reported approximately 20-second live coding response delay remains unmeasured and unresolved in this work.

Native status now distinguishes waiting for speakable agent text from synthesis and playback. It does not attribute all upstream waiting to the model: routing, tools, provider response and sentence gating can all contribute. No model/reasoning settings were reduced and no filler speech was introduced.

## Authorization and constraints

Browser planning answer, verbatim:

```text
next_step: Authorize one short playback of the PCM-fixed production path, you can use few playback do all necessary to eliminated the high latency. Check latest documentation!
```

Three explicit live-output invocations were performed: one preview phrase, one serial two-sentence check, and one overlapping two-sentence check. No microphone, live coding provider, volume/device-setting change, download of runtime/model assets, alternate speech engine, installer, or installed-binary replacement was used. The real production Kokoro worker retains its existing pack lease refresh; this authorized device probe is separate from isolated foundation verification. Tests use temporary roots and synthetic services/processes. Public documentation was fetched read-only.

The workspace was substantially dirty before this work. Baseline HEAD was `94ed16de502e98110498010b399b24659b17a63f`; aggregate changes are not attributable to this repair.

## Methods and primary sources

- Inspected `voice_speech_worker.rs::run_worker`/the former `speak_one`: each job synthesized, wrote PCM, and waited for player exit before the next job was received. This serialized CPU synthesis with acoustic playback.
- Inspected native `apply_agent_progress`, `drain_speech_status`, `dispatch_voice_submission`, Preview composition, and the shared hygiene gate. The host routes content deltas into the sentence gate; terminal-only output is a separate fallback without replay. No arbitrary early fragment or reasoning text was promoted to speech.
- Current [ONNX Runtime threading documentation](https://onnxruntime.ai/docs/performance/tune-performance/threading.html), fetched 68,679 bytes, SHA-256 `81929ca940c44dd51e08ffa457a2d2ffda35340848dc03b93851be28075d535c`. It distinguishes intra-op and inter-op parallelism, warns that parallel execution can hurt some models, and describes spinning/resource tradeoffs. The previous local one/two/four-thread experiment in [latency repair](voice-latency-repair.md) remains relevant. This repair does not change the two-thread inference configuration or adopt unmeasured accelerator/thread settings.
- Current [kokoro-onnx `create_stream` source](https://github.com/thewh1teagle/kokoro-onnx/blob/3596b26764286a7de9d90c363e988d50578918e5/src/kokoro_onnx/__init__.py#L348-L406), revision `3596b26764286a7de9d90c363e988d50578918e5`, fetched 15,552 bytes, SHA-256 `32602bee21b14ed161fdfd3c8b921b267782ad143a49c3ebd256c16c473808f5`. It documents background synthesis, bounded ahead-of-playback queuing, and cancellation that stops future batches rather than preempting the active native run. This is a comparison source, not a new Vesper runtime dependency or the frozen behavioral oracle.

Relevant upstream excerpt:

```python
# One slot for the batch being played, one for the batch synthesized
# ahead of it, so the producer cannot run away with the whole text
queue: asyncio.Queue[tuple[NDArray[np.float32], int] | Exception | None] = (
    asyncio.Queue(maxsize=1)
)
```

Vesper uses a **zero-capacity rendezvous**, rather than copying the Python queue literally: the producer may hold one computed successor while the consumer plays the current unit, but cannot start the third synthesis until handoff. Native adapters remain unchanged.

Research deviations: `web_search` failed with `auth unavailable`; direct HTTPS retrieval succeeded. The first raw GitHub path omitted `src/` and produced no source; the corrected path above was retrieved and pinned. The configured frozen Python path `/home/alex/Projects/Native GLM-5.2 Provider` was absent in this environment; it was not edited, and no impossibility or new feature-exclusion claim is based on that absence.

## Files and durable contracts

- `apps/agent-vesper-tui/src/voice_speech_worker.rs`: dedicated synthesis and playback lanes, rendezvous handoff, one prepared successor, maximum 16 MiB canonical PCM per prepared unit, flat PCM collection, sample-aligned bounded player writes, independent stage clocks, empty/oversized output refusal. Adapter-native allocations are separate from the prepared-PCM bound. Stop/selection replacement and player admission share a short lock, never pipe writes or drain waits. Queued and prepared work retain their generation; stale work cannot reopen the player. A stopped drain reports Stale, not Spoke. A new generation works normally.
- `apps/agent-vesper-tui/src/main.rs`: wait status based on the existing turn clock, with a focused binary regression. No new provider or inference policy.
- `apps/agent-vesper-tui/tests/voice_speech_pipeline.rs`: isolated subprocess PATH/HOME, synthesis-only WAV fixture and no-device player. Pins overlap, single lookahead, Stop, exact stale outcomes, fresh-generation recovery, FIFO completion, and accepted PCM counts.
- `apps/agent-vesper-tui/examples/speech_pipeline_receipt.rs`: manual two-sentence timing/listening probe; no sound without `--play-two`. `--serial` waits for first completion before enqueueing the second unit. Never run from ordinary CI.
- TUI, tests, examples and foundation `AGENTS.md`; this report, evidence index and voice PRD. The old authorized-playback record is corrected to retain the user's negative result rather than contradictory unanswered-listening claims.

ACP assessment: these are host-owned local audio scheduling and terminal status changes, not a new provider-neutral agent capability. ACP has no F9/device player owner; the existing justified terminal-only exclusion remains. No shared cognition, slash-command or provider behavior changed.

## Red → green regression

Command:

```sh
cargo test -p agent-vesper-tui --features voice-kokoro --test voice_speech_pipeline -- --nocapture
```

Before production edits:

```text
second synthesis must start before first player drains
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.07s
```

The first fixture player deliberately stays alive while the test waits for the second synthesis marker. The repaired producer can synthesize that unit without waiting for player exit. Stop kills the owned fixture before assertions complete. Final expanded regression:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
```

Additional unit checks cover PCM aggregation and its byte bound, empty audio refusal, cancellation during stream polling, and stale prepared audio rejection before opening even a nonexistent player.

## Authorized real-output receipts

### Cold preview

```sh
cargo run -p agent-vesper-tui --features voice-kokoro --example preview_latency_receipt -- --play-once
```

```text
t=0.000s adapter created
t=3.059s verified session ready
t=5.387s synthesis open complete
t=5.388s player spawned
t=5.388s PCM ready samples=54400
t=5.388s pipe write begins
t=6.309s pipe write complete BytesWritten(108800)
t=9.005s player settled Drained; not human listening evidence
```

Cold verification/session preparation took 3.059 s; synthesis took 2.328 s. This launcher uses the real adapter/player sequence, not the Settings modal or a model turn.

### Serial control and overlapping worker

```sh
cargo run -p agent-vesper-tui --features voice-kokoro --example speech_pipeline_receipt -- --play-two --serial
cargo run -p agent-vesper-tui --features voice-kokoro --example speech_pipeline_receipt -- --play-two
```

```text
Real selected-path Kokoro am_michael; serial=true; no microphone/provider; transport is not audibility.
t=0.000s Preparing voice runtime · 0.0s
t=2.580s Synthesizing voice · 0.0s
t=4.819s Sending PCM to player · 0.0s
t=5.869s Waiting for player drain · 0.0s
t=5.869s Progress { segment: 1, through_bytes: 112800 }
t=8.574s Spoke { segment: 1, samples: 56400 }
t=8.584s Synthesizing voice · 0.0s
t=10.986s Sending PCM to player · 0.0s
t=12.272s Waiting for player drain · 0.0s
t=12.272s Progress { segment: 2, through_bytes: 119200 }
t=14.850s Spoke { segment: 2, samples: 59600 }
Real selected-path Kokoro am_michael; serial=false; no microphone/provider; transport is not audibility.
t=0.000s Preparing voice runtime · 0.0s
t=1.898s Synthesizing voice · 0.0s
t=4.142s Synthesizing voice · 0.0s / Sending PCM to player · 0.0s
t=5.141s Synthesizing voice · 1.0s / Waiting for player drain · 0.0s
t=5.141s Progress { segment: 1, through_bytes: 112800 }
t=6.516s Waiting for playback slot · 0.0s / Waiting for player drain · 1.4s
t=7.841s Sending PCM to player · 0.0s
t=7.841s Spoke { segment: 1, samples: 56400 }
t=9.114s Waiting for player drain · 0.0s
t=9.114s Progress { segment: 2, through_bytes: 119200 }
t=11.684s Spoke { segment: 2, samples: 59600 }
```

Serial first-drain to second-write-stage observation: **2.412 s**. Overlap: both appeared in the same 10 ms polling interval. This is not a measured acoustic gap. The first-send-stage to final completion spans were 10.031 s and 7.542 s, a 2.489 s local reduction; whole-run comparisons include differing cold setup costs and are not controlled cross-hardware benchmarks. CPU inference itself still takes roughly 2.2–2.4 s for these phrases.

Human answer, verbatim:

```text
listening: first test was with delay between sentence but second test was without delays.
```

This confirms the perceived difference between the two worker checks, not separate acceptance of every Preview interaction. Later flat-buffer consolidation, extra regression coverage and selection-error bookkeeping were verified device-free; the listening receipt precedes those final small edits. No additional sound was needed to rerun deterministic pipeline/order/signal checks.

## Final verification

Commands:

```sh
cargo test -p agent-vesper-tui --features voice-kokoro --lib --test voice_speech_pipeline --test r3_speech_worker --test voice_playback_diagnostics --quiet
cargo test -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui voice_wait_status --quiet
cargo clippy -p agent-vesper-tui --all-targets --features voice-kokoro -- -D warnings
cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui
cargo check -p agent-vesper-tui --bin agent-vesper-tui
cargo check -p agent-vesper-tui --features voice-conversation --lib
cargo xtask architecture
python3 apps/agent-vesper-tui/tests/r3_loop_pty.py
python3 apps/agent-vesper-tui/tests/r3_loop_pty.py target/debug/agent-vesper-tui "$HOME/.local/share/agent-vesper/voice-venv/bin/python" af_heart balanced
```

Exact final test receipts, respectively library, existing worker, diagnostics, pipeline and wait status:

```text
test result: ok. 276 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 155 filtered out; finished in 0.05s
```

Scoped Clippy completed successfully with `-D warnings` (`Finished dev profile ... in 2.48s`). Default binary and conversation-only checks, voice-capable build, and architecture gate succeeded:

```text
architecture boundaries validated for 30 packages
```

Final direct and VRO native F9 fixture receipts:

```text
LATENCY: recorder process observed=250.5 ms; stop-to-first-PCM=1651.0 ms; complete fixture reply=4257.8 ms; includes PTY polling, synthetic STT/provider, real Kokoro; not real end-to-end latency
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52000, 124800]; peaks=[22327, 13045]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
LATENCY: recorder process observed=250.4 ms; stop-to-first-PCM=2158.2 ms; complete fixture reply=5260.4 ms; includes PTY polling, synthetic STT/provider, real Kokoro; not real end-to-end latency
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro af_heart (balanced) -> fixture player; PCM bytes=[50400, 108000]; peaks=[21248, 14919]; provider requests=1
Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.
```

Earlier post-repair PTY runs measured 2191.1/2079.3 ms stop-to-first-PCM and 6261.4/4758.8 ms complete fixture reply. Timing variability is retained; none of these substitutes for a live-provider 20-second-turn measurement. Amplitude and wire assertions passed on both runs.

## Final audit, deviations and open acceptance

- Re-derived the two-unit bound from the rendezvous, FIFO playback from the single consumer, generation checks before/after synthesis and writes, and admission serialization with Stop. Expanded regression proves Stop recovery and exact terminal outcomes. Flat PCM storage avoids per-frame metadata growth. Oversized or empty synthesis is visible failure, not silent truncation or Spoke.
- The pre-repair overlap regression is behavioral evidence, not a source-pattern check. New defensive tests are current-code checks; they are not advertised as additional pre-repair red receipts.
- Corrected stale worker commentary that treated earlier reproduction as complete engine exoneration. Corrected historical unanswered-listening references: Alex explicitly did not hear the original pre-scaling attempt; later corrected playback is distinct evidence.
- **Initial live coding reply latency remains open.** Waiting for speakable text, real transcription, provider/reasoning/tool time and cold setup require separate end-to-end measurement. One lookahead cannot hide synthesis longer than the current sentence's playback or lack of a next validated sentence.
- No live microphone/F9-provider turn, Settings interaction listening test, fresh install, pronunciation campaign, long-form acoustic measurement, full workspace suite, MSRV or cross-platform/release matrix was performed. No release readiness claim.
- The already-running/installed application was not replaced. `target/debug/agent-vesper-tui` is the verified workspace build.
- DOX pass updates nearest TUI/tests/examples/foundation ownership and evidence links. Root, apps parent and docs parent remain unchanged: existing latency, permissions and reporting contracts still apply and no new domain boundary or child index entry was introduced. No production crate contracts changed. Link and whitespace verification is recorded in the follow-up closeout.
