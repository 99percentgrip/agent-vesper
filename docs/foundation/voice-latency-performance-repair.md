# Voice latency and performance repair after native acceptance

## Objective and status

Reduce the Preview and first-spoken delays reported in the [initial native retest](voice-user-latency-acceptance.md) without changing Alex's selected model/reasoning, bypassing speech hygiene, adding filler speech, opening a microphone/speaker, or weakening Stop and bounded buffering.

**Code repair implemented and locally verified; qualitative native improvement confirmed; quantitative acceptance remains OPEN.** The candidate now starts synthesis from a smaller first piece for ordinary speech units and keeps one reusable Preview worker for the lifetime of the Natural Voice pack screen. After running the release candidate, Alex reported **“ok its way faster then before.”** No per-attempt timings or the complete interruption/error matrix accompanied that result, so it confirms material improvement but does not establish a numeric threshold or close the full VRO-17 phase.

## Diagnosis

Read-only tracing and device-free measurements separated three contributors:

1. **Preview rebuilt the runtime after every click.** `settings_host.rs::preview_neural_voice` constructed and dropped a new `SpeechWorker` for each attempt. The worker performed pack assessment and ORT session preparation only after Preview was selected.
2. **Ordinary first sentences still buffered whole.** `voice_speech_worker.rs::speech_pieces` preserved every hygiene-approved unit up to 120 characters. The fixed Preview phrase and many normal first sentences therefore could not write PCM until their entire waveform was generated.
3. **F9 includes upstream time that local TTS cannot remove.** Prior live evidence first reached synthesis at 17.518 s and PCM at 26.990 s. The latest user retest observed 20/40/20 s to first speech. Routing/provider/reasoning and sentence arrival remain separate from local synthesis; no model/reasoning setting was reduced and no provider claim is inferred.

Pre-repair device-free receipts in this work unit:

```text
assessment 0: 1644.363 ms
assessment 1: 0.081 ms
assessment 2: 0.036 ms
engine load: 257.274 ms
synthesis 0: 2190.831 ms, 54400 samples
synthesis 1: 2195.953 ms, 54400 samples
synthesis 2: 2181.176 ms, 54400 samples

warm whole-unit synthesis open: 8.763s; 188 text bytes; no playback
t=2.799s first observed Sending PCM stage; not acoustic onset
t=2.810s first PCM observed by fixture player
```

These show a cold per-process pack check, a smaller session-load cost, and full Preview-phrase synthesis around 2.2 s after readiness on this run. They do not explain all of the user's observed 10–15 s Preview delay or 20–40 s F9 delay.

## Implementation

### Earlier first PCM

`apps/agent-vesper-tui/src/voice_speech_worker.rs` now:

- leaves only units of at most 32 Unicode scalar values whole;
- for longer, already hygiene-approved units, targets a first word-boundary piece of 28 characters with a minimum boundary of 12;
- targets successor pieces of 48 characters with a minimum boundary of 24;
- preserves exact UTF-8 reconstruction and never invents punctuation/content;
- retains one player stream, one prepared lookahead, 16 MiB per-piece PCM bound, cumulative receipts and exactly one terminal result for the original segment.

The 48-character successor target is deliberate: making the first piece shorter without also bounding its successor reproduced a software handoff risk because the second inference could outlast the first piece's acoustic duration. Smaller successors keep preparation close to playback while preserving bounded one-piece lookahead.

The unit regression was written first and failed on the prior implementation:

```text
medium preview text must reach first PCM without whole-unit buffering
test result: FAILED. 0 passed; 1 failed
```

It now passes and pins exact reconstruction, a multi-piece fixed Preview phrase and a first piece no longer than 28 characters.

### Reusable Settings Preview worker

`apps/agent-vesper-tui/src/settings_host.rs` now creates one `SpeechWorker` when the installed Natural Voice pack screen opens. The worker prepares while the menu is visible and is reused for immediate repeat Preview actions. `preview_neural_voice` only enqueues and drains; it no longer constructs and destroys the runtime around each click. Leaving the pack screen drops the worker. Preview remains explicit: preparation opens no speaker, and only the Preview action enqueues audio.

Install, Repair/Verify, Remove, selected engine/voice, grouped Settings Save behavior and F9 composition are otherwise unchanged. The worker still uses the selected draft voice for this screen, the production player and the real Kokoro adapter. No persistent global 815 MiB engine cache was introduced.

### Evaluation helper and regression

- Added `apps/agent-vesper-tui/examples/preview_first_pcm_receipt.rs`: installed Kokoro + production worker + temporary no-device sink, reporting worker readiness and two repeat Preview attempts.
- Extended `apps/agent-vesper-tui/tests/voice_speech_pipeline.rs` so cumulative progress and one terminal settlement remain correct for the increased piece count without pinning an incidental count.

## Performance evidence

Representative post-repair device-free receipts varied with concurrent machine load. The best uncontended debug run for the fixed 188-byte long unit was:

```text
warm whole-unit synthesis open: 8.592s; 188 text bytes; no playback
t=1.624s first PCM observed by fixture player
t=1.624s Progress { segment: 1, through_bytes: 81600 }
t=3.965s Progress { segment: 1, through_bytes: 200000 }
t=6.042s Progress { segment: 1, through_bytes: 304800 }
t=8.464s Progress { segment: 1, through_bytes: 426400 }
t=9.800s Progress { segment: 1, through_bytes: 494400 }
t=9.810s Spoke { segment: 1, samples: 247200 }
```

Compared with the prior recorded 2.733 s first PCM on the same fixed text, this run reached fixture PCM **1.109 s / 40.6% earlier**. Across post-repair runs observed in this work unit, long-unit first PCM ranged approximately **1.62–2.59 s** as machine load varied. Total synthesis/delivery can increase because shorter pieces trade compute efficiency for onset and bounded overlap; the candidate does not claim lower total compute.

Release-mode fixed Preview helper receipt:

```text
worker_ready_s=0.332
attempt=1 enqueue_to_first_pcm_s=1.559
attempt=1 enqueue_to_settled_s=2.807 samples=70800
attempt=2 enqueue_to_first_pcm_s=1.569
attempt=2 enqueue_to_settled_s=2.817 samples=70800
```

This helper deliberately waits for worker readiness before enqueueing. In the UI, preparation begins when the pack screen opens, so ordinary menu dwell can overlap the readiness cost. It is not an acoustic-onset measurement, and it does not prove a click-to-audible threshold on Alex's speakers.

A four-thread ORT experiment was measured and rejected. It improved the isolated fixed Preview synthesis by roughly 10% in one run but worsened/varied long-unit onset and whole-unit time under contention. Production remains at the established bounded two intra-op threads; no unverified CPU-aggressive setting was retained.

## Native user retest

Alex launched the release candidate from the repository and then reported, verbatim:

> ok its way faster then before

This is current user-operated device evidence that the candidate materially improved perceived latency relative to the previously tested build. It does not provide cold/warm Preview seconds, three F9 timings, transcript timing, interruption recovery or error-path results. Therefore:

- **Latency improvement direction:** user-confirmed.
- **Candidate implementation:** implemented and locally verified.
- **Quantitative latency acceptance:** still open.
- **Full VRO-17 phase acceptance:** still open because repeated-interruption, NPU STT/TTS and PR-5 gates remain separate.

## Verification

Commands and exact final receipts:

```text
cargo test -p agent-vesper-tui --features voice-kokoro --lib speech_pieces_preserve_text_utf8_and_prefer_natural_boundaries -- --nocapture
1 passed; 0 failed

cargo test -p agent-vesper-tui --features voice-kokoro --test voice_speech_pipeline -- --nocapture
1 passed; 0 failed

cargo test -p agent-vesper-tui --features voice-kokoro --test r3_speech_worker --test voice_playback_diagnostics --quiet
6 passed; 0 failed
2 passed; 0 failed

cargo test -p agent-vesper-tui --features voice-kokoro --lib --quiet
278 passed; 0 failed

cargo clippy -p agent-vesper-tui --all-targets --features voice-kokoro -- -D warnings
exit 0

cargo check -p agent-vesper-tui --features voice-kokoro --example preview_first_pcm_receipt
exit 0
```

`cargo fmt --all -- --check` remains non-green because pre-existing modified `apps/agent-vesper-tui/src/ui.rs` lines are not rustfmt-formatted. The four files changed by this repair were formatted directly with `rustfmt --edition 2024`; unrelated UI work was not rewritten.

No microphone, speaker, live provider, installer, saved setting, model download, NPU workload or installed Vesper binary was operated. No live-provider rerun was authorized or performed. The existing installed application was not replaced.

## Files

Production:

- `apps/agent-vesper-tui/src/voice_speech_worker.rs`
- `apps/agent-vesper-tui/src/settings_host.rs`

Verification/evaluation:

- `apps/agent-vesper-tui/tests/voice_speech_pipeline.rs`
- `apps/agent-vesper-tui/examples/preview_first_pcm_receipt.rs`

Contracts/evidence:

- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-tui/tests/AGENTS.md`
- `apps/agent-vesper-tui/examples/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/voice-first-speech-and-npu-assessment.md`
- `docs/foundation/voice-user-latency-acceptance.md`
- `docs/voice-oracle-extraction-prd.md`
- `docs/migration-status.md`

## Deviations and unresolved acceptance

- Background delegation failed because the configured OpenAI model was no longer in the current account model list; investigation continued directly without weakening scope.
- Alex performed a real-device candidate retest and confirmed it was “way faster,” but no instrumented or approximate seconds were supplied. Fixture PCM remains software delivery, not the first audible word.
- The candidate does not eliminate upstream provider/reasoning/routing delay before speakable text exists.
- The fixed Preview phrase still requires local inference; warm worker reuse removes repeated preparation, not synthesis itself.
- Native user acceptance remains required: cold/warm Preview; three F9 turns with recorder/transcript/first-audible timestamps; long response continuity; barge-in recovery; visible error preservation.
- Native NPU STT and NPU TTS remain separate unimplemented/unaccepted gates.
- Workspace/MSRV/five-platform/release/install gates were not run.

## Readiness effect

The latency candidate is **IMPLEMENTED, LOCALLY VERIFIED, AND QUALITATIVELY USER-CONFIRMED AS WAY FASTER**. VRO-17 as a whole remains **OPEN — DO NOT MARK THE FULL PHASE ACCEPTED** because the retest supplied no repeat timings and did not execute every interruption/error/NPU/PR-5 gate. The earlier 10–40 second result applies to the superseded tested build; it is not silently treated as current candidate performance.

## Closeout receipts

```text
rustfmt --edition 2024 --check [four changed Rust files]: exit 0
voice_relative_links_checked=157
voice_missing_relative_links=0
trailing_whitespace_issues=0
git diff --check: exit 0
```

Source identities at closeout:

```text
aa236e7676f4f2140afec0bfb951e4dfea31e0a9905e8a425f6b0bbbb43e04d8  apps/agent-vesper-tui/src/settings_host.rs
752c347654769e014199200af0d206fc793726bbeecb29825135cffce5213efd  apps/agent-vesper-tui/src/voice_speech_worker.rs
b5cd4227677919272ad58928d1308eceb586c07d799a446d87a26daf35589d57  apps/agent-vesper-tui/tests/voice_speech_pipeline.rs
b2f638906371a14141e674039212a997b9f6f1a14382d1f9fe6761ba2f35e970  apps/agent-vesper-tui/examples/preview_first_pcm_receipt.rs
```

DOX pass: the TUI, tests, examples and foundation owning contracts were updated. Root, `apps/AGENTS.md` and `docs/AGENTS.md` remain unchanged because repository/application/documentation ownership boundaries did not change. The workspace contains extensive pre-existing modified and untracked work; this report attributes only the files and commands listed above to this repair.
