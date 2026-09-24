# Speech continuity and empty-phoneme reconnaissance: pause timeline, error fixtures, and minimal remedy proposals

> **Scope:** bounded reconnaissance after Alex's device feedback on the
> multi-turn repair candidate: speech works again, but long replies have
> excessive pauses between words/sentences/paragraphs, and some output fails
> with `speech failed: invalid voice input: text produced no pronounceable
> phonemes`. This unit measures and reproduces; **no production code was
> changed** (diagnostic examples only). Remedies are proposed, not applied.

## Build identity and baseline (§1)

```text
commit:  8f258ba28f4ea2f749526fb32b5b180ecc7dcead (dirty workspace, 94 entries —
         unchanged by this unit; only new example files + this report)
tested exe (Alex's candidate): target/voice-candidates/agent-vesper-tui-multiturn-repair
         sha256 7bd31168b25817b495406581982dad0f83c787cbb2dc4cb7ab690c374a6fce6a
         = current target/release/agent-vesper-tui (verified identical digest)
prior candidates preserved: agent-vesper-tui-voice-cpu-acceptance (d633b4c3…)
engine:  Kokoro model_quantized.onnx (CPU, 2 intra-op threads — unchanged)
voice:   am_michael; phonemizer: /usr/bin/espeak-ng (1.2.16 family)
routes:  STT = shared sidecar faster-whisper base/int8 CPU; TTS = Kokoro CPU;
         accelerator registry EMPTY (verified `registered_routes()` returns none)
         ⇒ both CPU and Automatic resolve to CPU. No NPU evidence exists or is claimed.
```

Requirement mapping: R5 (prompt onset + complete natural output), R6/R8
(interruption/settlement preserved — untouched), R11 (truthful errors — the
phoneme error is correctly surfaced, its trigger is the defect), R12 (no
private text in telemetry — all fixtures synthetic), R15 (playback evidence
receipts — reused, not weakened), R16 (CPU preserved; no acceleration claim).
The 28/48-char piece policy and one-successor lookahead are **implementation
decisions** (from the latency repair), not product requirements; replacing
them is in scope for the next repair if evidence supports it.

### Ownership chain (traced, no duplicates found)

assistant delta → `HygieneGate` (sentence units; whitespace-collapsed;
protected spans skipped) → `SpeakUnit` per sentence → `SpeechWorker`
producer thread (`speech_pieces` 28/48 word/clause cuts → `prepare_one` =
phonemize + ONNX + resample) → rendezvous `sync_channel(0)` → player thread
(`write_piece` → `PlaybackOwner.begin_stream`/`push_pcm`/`end_stream`) →
one `aplay` child per SEGMENT (persists across that segment's pieces;
closed/reopened between segments). One engine session per worker; one
playback lane; no duplicate schedulers or sessions; Settings-refresh worker
replacement already no-ops on unchanged selection.

## §2 Where the pauses occur (measured, one persistent session, paced sink)

Production worker + real Kokoro + a sink paced at the canonical 32 kB/s
(`continuity_pause_receipt`, release profile; **not** an instant drain):

```text
[ordinary]       first PCM t+2.60s; total 6.42s; audio 4.05s
[commas]         first PCM t+1.83s; total 13.01s; audio 11.38s; pipeline-RTF 1.14
[multi-sentence] first PCM t+1.95s; total 13.10s; audio 11.38s; pipeline-RTF 1.15
[long-answer]    first PCM t+2.34s; total 43.00s; audio 41.05s; pipeline-RTF 1.05
consumer read-trace: 321 reads, p50=p90=p99=0.128s (exact pace), 0 gaps >0.2s
```

Engine-level per-piece timing on the same long passage
(`piece_rtf_receipt`, quiet threshold ±400/32768 ≈ 1.2% FS):

```text
current 28/48 pieces (12): infer 26.1s / audio 41.0s → aggregate RTF 0.64 (warm)
  per-piece RTF: 0.63–0.66 warm; 0.96–0.99 on the first (cold) pieces
  per-piece generated quiet: lead 0.25–0.29s, trail 0.26–0.46s
whole sentences (5):        infer 24.8s / audio 37.8s → aggregate RTF 0.66
```

### The four contributions, separated

1. **Late source text / gate wait:** not measurable here (fixtures are
   fully available); on-device this adds the provider's own pacing. Not the
   dominant term in these fixtures.
2. **Synthesis/dispatch starvation:** WARM production keeps pace
   (RTF≈0.64; the paced sink saw zero read gaps — the rendezvous + 64 KiB
   pipe ≈2.0 s of audio absorbed everything). COLD pieces (RTF≈0.97) leave
   no margin: the successor takes nearly the piece's whole duration, so any
   variance drains the pipe at each boundary. This matches pauses being
   worst early in a reply and improving later (warm cache) — consistent
   with Alex's report.
3. **Generated silence inside waveforms at artificial boundaries (the
   dominant measured cause):** every piece begins and ends with a model
   fade-in/out quiet run (~0.25–0.46 s each side). Consecutive pieces
   CONCATENATE these: measured **7.2 s of stacked quiet across 11
   mid-sentence boundaries (avg 0.66 s each)** in the 41 s passage —
   silence inserted at comma/space positions where no natural pause
   exists. Whole-sentence pieces stack only 2.9 s across 4 boundaries,
   which coincide with natural sentence pauses (added silence ≈ 0).
   Amplitude thresholding is an approximation (fricatives/breath are not
   silence); the stacking arithmetic uses only run lengths at piece edges,
   where the fade shape is unambiguous in these waveforms.
4. **Inter-segment close/reopen:** one `aplay` per sentence-segment; the
   spawn + first-write cost sits between segments, adding to (not
   replacing) natural sentence pauses. Not separately quantified on-device;
   bounded by the same mechanisms as before.

**Corrected attribution:** the historical ~2.2 s figure was full
Preview-phrase synthesis after readiness, NOT a per-piece constant.
Current per-piece cost is 1.5–2.6 s warm for 27–49 chars (2.4–4.1 s audio).
RTF here is generation-time/audio-duration per piece as measured above.

**Documentation correction applied:** the multi-turn report's EPIPE wording
now states that at an ordinary pipe write EPIPE means *no read-end
descriptors remain* (in this architecture: the child exited), with the
reap-vs-unreaped distinction established by the paired experiment rather
than the errno; ALSA PCM underrun errors from a live player remain a
separate stderr-classified layer.

## §3 Empty-phoneme reproduction (exact constructor, minimal fixtures)

Constructor: `phonemize.rs::ids_from_phonemes` → `VoiceError::InvalidInput(
"text produced no pronounceable phonemes")` when the vocab maps every
phoneme character to nothing. Reached from `KokoroEngine::synthesize_blocking`
(one call per PIECE) → the worker fails that segment's remaining pieces and
surfaces `speech failed: …` once.

Real-gate + real-phonemizer + real-vocab matrix (`phoneme_shape_matrix`,
`phoneme_char_matrix`):

- **22 characters yield ZERO ids alone:** `^ ~ | \` # @ $ % & * _ + = < > [ ] { } \\ / -`
  (hyphen included; `—`/`…` are fine: 1 id).
- **Embedded in prose, all are harmless** (espeak speaks around them).
- **Full production-path reproducer:** a numbered list streams
  `"Steps.\n\n1. Do the first thing\n2. …"` → the gate emits
  **`"1."` as its own hygiene unit** → `ids_from_phonemes` fails →
  **exactly Alex's error.** Confirmed twice (`numbered list`,
  `numbered dense`). Table rows, bullets, headings, rules, versions,
  decimals, accents all pass. Word-boundary piece splits cannot strand a
  lone symbol (exhaustive search: 0 cases) — the trigger is the GATE's
  sentence boundary, not the piece split.
- Classification is distinct from: empty text (gate drops it), phonemizer
  nonzero exit / truncation (different errors), cancellation (Stale).

**Shared defect? No — independent.** The pause mechanism is engine fade at
piece boundaries + cold RTF; the phoneme error is a gate-level unit with no
speakable content. Fixing either does not affect the other.

### Proposed minimal repair (next unit, not applied)

The gate already carries marker machinery (`HygieneMarker::SkippedSpan`,
carried markers). Minimal correction at the **hygiene boundary**: a unit
that reduces to formatting-only content (numbered/bullet markers, table
separator lines, heading hashes, rules) should be classified positively —
e.g. a `FormattingOnly` marker — and omitted from `SpeakUnit` emission the
way all-omitted input already emits nothing, WITHOUT fake audio or playback
ack; it must not strand a later valid segment (markers already carry
forward). Meaningful text that loses phonemes stays an actionable failure.
No ASCII filter, no number deletion, no blanket swallow.

## §4 Remedy comparison (isolated, no production change)

| Variant | Boundaries | Stacked quiet | Agg RTF | First-piece audio (onset) |
|---|---|---|---|---|
| current 28/48 | 11 | **7.2 s** | 0.64 | 2.67 s |
| whole sentences | 4 | 2.9 s (at natural pauses → ≈0 added) | 0.66 | 2.73 s |

- **Throughput vs variability:** warm production sustains RTF 0.64 — the
  deficit is not sustained; cold-start RTF≈0.97 plus per-boundary fade is
  the audible problem. More prebuffer absorbs the cold transient but
  cannot help the stacked-silence term at all.
- **Longer natural units (sentence-level pieces)** are the evidence-backed
  minimal change: they remove ~4.3 s of inserted silence in this passage
  with the same aggregate RTF and essentially unchanged onset (first
  sentence ≈ first piece today: 2.73 s vs 2.67 s playable after one
  inference). Model context: 510 phoneme IDs (whole 156-char sentence
  measured fine). Onset stays protected because the FIRST piece can remain
  clause-sized (~28 chars) while successors become sentence-sized.
- **Bounded audio-duration preparation** (e.g. keep ≤~6 s of prepared
  audio ≈ 2 pieces ≈ 384 kB, far under the 16 MiB/piece bound) would cover
  the cold-start transient; only needed if sentence-level pieces plus the
  pipe still starve on device. Not proposed as the first step.
- **Threads:** the earlier four-thread ORT experiment was measured and
  REJECTED (better isolated Preview, worse/varied long-unit onset under
  contention). No thread change proposed; CPU two-thread setting stands.
- Not proposed under any evidence: silence insertion, waveform trimming,
  punctuation removal, error suppression, answer shortening (a
  presentation-policy change would be a separate voice-scoped host
  instruction decision in `voice_turn_instruction.rs`), engine/daemon/NPU
  work.

## Observed vs unverified

- **Observed (fixtures/engine):** pause stacking arithmetic, warm/cold RTF,
  paced-sink continuity, zero-id character set, the `"1."` reproducer,
  gate unit shapes, absence of scheduler/session duplication, registry
  emptiness.
- **Unverified (device-only):** actual acoustic gap lengths on Alex's
  speakers, on-device provider pacing, real PipeWire xrun behavior (prior
  unit), microphone-side transcript accuracy. A paced sink and PCM counts
  certify nothing about audible continuity — Alex's listening verdict
  remains the acceptance boundary.

## Proposed red-first acceptance for the next repair

1. Production Preview/F9 text flow; long multi-sentence/paragraph reply
   with a paced consumer and realistic synthesis variability: no
   inter-piece starvation beyond the pipe's 2 s cover in the warm regime;
   stacked generated quiet at mid-sentence boundaries ≤ natural pause
   envelope (quantified by the same quiet-run receipts).
2. Preserved natural punctuation; no dropped/duplicated words
   (concatenation identity test per unit).
3. Formatting-only units (numbered lists, tables, rules, headings) produce
   no `SpeakUnit`, no failure, and do not strand later segments; meaningful
   text with no phonemes STILL fails visibly.
4. No dead-pipe regression (the multiturn suite stays green); repeated
   Stop then next-turn recovery intact.
5. First-piece onset within the recorded 1.6–2.9 s envelope (not
   regressed by the change); performance receipts separate from Alex's
   listening verdict.

## Status

- Audible output: observed working again (Alex). **Natural continuity of
  long replies: FAILED/OPEN** (mechanism established above). **Empty-phoneme
  error on formatting units: reproduced, root cause established, repair
  proposed (not applied).** Earlier positive onset feedback and all prior
  test scopes preserved. No production code changed in this unit.

## Cleanup

All diagnostic examples are isolated (`examples/` files + Cargo manifest
entries only); temp fixtures removed (0 residue); installed pack untouched
(118 009 413 B); disk 262.5 GiB free; no downloads, devices, or settings
changes.
