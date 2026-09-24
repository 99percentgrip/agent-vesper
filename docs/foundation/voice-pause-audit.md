# Voice pause audit — three identified culprits, no code changes

> **Scope:** investigation only. Alex reported unnatural pauses "between
> some letters" and delays with gaps between words in the F9 voice test.
> This report identifies the exact mechanisms from code and reproduced
> offline synthesis; no production code, setting, or asset was changed.

## Objective, status and readiness effect

Identify the culprit(s) for the unnatural pauses Alex heard when the F9
voice test spoke the one-sentence test reply.

**Audit complete — three concrete culprits identified with reproduced
evidence.** The perceived pauses decompose into distinct mechanisms with
distinct owners and fixes. No repair was applied in this work unit
(Alex asked for the culprit and a report, not a fix).

## What actually spoke (engine/voice truth)

The saved workspace scope is real: `/home/Alex/Projects/agent-vesper/
.agent-vesper/config.toml` contains `[voice] tts = "voice-kokoro"`,
`voice = "am_michael"` (read verbatim during this audit). So the F9 path
resolved to the **Kokoro neural pack** (`EngineSelection::Neural`),
pack root `~/.local/share/agent-vesper/voice-pack` with a verified
`pack.json` at pinned revision `1939ad2a…`. The baseline espeak-ng
subprocess engine was measured for comparison only.

## The full text-to-sound path (what the audio passes through)

1. Assistant text → `HygieneGate` (sentence units; em-dash and hyphens
   pass through unmodified — punctuation is preserved on purpose).
2. Each sentence unit → `ConversationHost::speak_unit` → `SpeechWorker`
   queue.
3. `speech_pieces()` (voice_speech_worker.rs) splits units >32 chars:
   first piece ≈28 chars, successors ≈48 chars, always at whitespace.
4. `KokoroEngine::synthesize_blocking` per piece: normalize → espeak-ng
   IPA → phoneme IDs → ONNX inference at 24 kHz → convert to 16 kHz PCM.
5. Pieces concatenated into ONE aplay stream; bytes pipe through with no
   gap of our creation — the pipe is continuous.

## Culprit 1 — every synthesized piece carries ~400 ms leading and
~770 ms trailing silence (the big "gaps between words")

Measured offline by synthesizing the exact test sentence
("Confirmed — this is a one-sentence reply to verify the test is
working.") through the real engine with the installed pack, dumping the
canonical PCM, and window-analyzing it in 10 ms frames (threshold 15%
of peak):

- Whole sentence: **400 ms of silence at the start**, 770 ms trailing;
  interior gaps of 380/150/140 ms; total 5.85 s at only **45% speech
  ratio**.
- The production splitter cuts this sentence into 3 pieces
  ("Confirmed — this is a " / "one-sentence reply to verify the test is
  " / "working."). Synthesizing the exact pieces and concatenating
  reproduces the audible stream: **1.24 s and 1.23 s interior gaps**
  plus 590 ms final tail = the long dead air between words/phrases.

Mechanism: Kokoro pads each inference with leading/trailing silence;
those pads are artifacts of the model's output envelope, not of our
pipeline. When the unit is split into pieces, every piece carries its
own pads: piece boundaries accumulate two overlapping silence ramps
(trailing pad of piece N + leading pad of piece N+1), which the ear
hears as a long pause. The aplay pipe is continuous — these gaps are
baked into the PCM itself.

Owner: `vesper-voice-kokoro` engine output + the TUI worker's piece
splitting interacting with those pads. Candidate fixes (not applied):
trim leading/trailing near-silence from each piece's PCM before piping,
or raise the piece thresholds so fewer pieces are synthesized
per sentence.

## Culprit 2 — punctuation tokens ("—", and "hyphen-joined" words) insert
model-level pauses between words

The em-dash in my test reply — and any hyphenated word like
"one-sentence" — reaches the model as kept punctuation. In
`phonemize.rs`, `KEPT_PUNCT` includes `;:,.!?—…""()«»`; kept sections
pass verbatim into the phoneme stream and are mapped to model input IDs
(the Kokoro vocab has IDs for `—` and `-`; upstream behavior). The
model inserts a prosodic break wherever those break tokens appear.

Measured: synthesizing "Confirmed — this is a…" vs the same sentence
with the dash removed changes the interior gap at that position
(380 ms with dash → 130 ms plain; comma also 390 ms). espeak-ng `--ipa`
emits a **newline** at comma/dash, and the kept "—" character is
re-emitted into the stream; both effectively act as pause tokens
(the newline is stripped by vocab mapping, but the "—" maps to a
pause/break ID). Reproduction receipts:

```text
$ printf '%s' "Confirmed — this is a " | espeak-ng --ipa -q -v en
kənfˈɜːmd
ðɪs ɪz ˈeɪ
$ printf '%s' "one-sentence" | espeak-ng --ipa -q -v en
wˈɒnsˈɛntəns
```

The 150 ms gap at "reply|to" and 380 ms at "this is a|one-sentence"
aligns with the model inserting breaks at punctuation/prosodic
boundaries — consistent with Alex hearing pauses "between some letters"
of words like "one-sentence" (the hyphen reads as a mid-word pause:
"wˈɒn … sˈɛntəns").

Owner: `vesper-voice-kokoro` phonemizer policy. Candidate fix (not
applied): speech-side normalization that drops/converts the em-dash
break token before ID mapping (e.g. treat "—" as a comma with a shorter
break or strip it), and similar handling for hyphens inside words.
Chat text stays canonical; only the pronunciation input changes.

## Culprit 3 — genuine computation/network gaps before speech begins
(the "delay" part of the complaint)

Structurally verified from code (not re-measured live in this audit):
before the first sound there is (a) provider/model latency to produce
the sentence, (b) ~4.2 s CPU synthesis measured for this very sentence
in a debug build (`synthesis: 4232.4 ms` receipt), and (c) per-piece
serialization through the bounded worker. Earlier reports
([voice-user-latency-acceptance.md](voice-user-latency-acceptance.md),
[first-speech and NPU assessment](voice-first-speech-and-npu-assessment.md),
[latency performance repair](voice-latency-performance-repair.md))
record multi-second live latency and its partial repairs; those remain
the state of the art here. The pauses *between* words are culprits 1–2;
the delay *before* words is culprit 3.

Owner: model/provider latency + CPU inference speed; tracked in the
existing voice-latency workstreams.

## Methods and exact evidence

Commands (reproduction, offline, no devices, no downloads):

```text
cargo run -p vesper-voice-kokoro --features ort --example pause_investigation
# (temporary example, deleted after evidence capture; it synthesized the
#  exact test sentence + punctuation variants through the real installed
#  pack and dumped canonical 16 kHz s16 PCM to /tmp)
python3 (10 ms window peak-threshold gap analysis over the dumped PCM)
printf '%s' "<piece>" | espeak-ng --ipa -q -v en
```

Gap tables (start_ms, duration_ms), threshold 120 ms:

```text
whole sentence (1 piece)  : (0,400) (520,140) (1250,380) (1930,150) (5080,770)
production 3-piece concat : (0,430) (520,140) (1360,1240) (3310,140) (4650,160) (4970,1230) (6610,590)
plain (dash removed)      : (0,400) (530,130) (1320,410) (4980,720)
comma variant             : (0,400) (530,130) (1310,390) (3440,120) (5110,790)
```

Code sources pinned: `apps/agent-vesper-tui/src/voice_speech_worker.rs`
(`speech_pieces`, thresholds 32/28/48), `crates/vesper-voice-kokoro/src/
phonemize.rs` (`KEPT_PUNCT` including `—`, section split, espeak IPA
newline behavior), `crates/vesper-voice-kokoro/src/vocab.rs` + installed
`tokenizer.json` (single-char vocab: `—` has an ID → break token),
`apps/agent-vesper-troom/...` n/a (host path: `voice_conversation.rs`
`speak_unit` → worker → aplay per-piece pipe, one continuous stream).

## Deviations

- Live audio playback was NOT performed (Alex's current message asked
  only to find the culprit and provide a text report; earlier
  authorized listening checks exist in prior reports).
- The temporary reproduction example was deleted after evidence capture;
  the workspace retains no new artifacts other than this report.
- The measured 4.2 s synthesis figure is a debug-build figure; release
  builds are faster (prior reports record ~1.5–2.6 s first-PCM), but the
  *pattern* of pads + break tokens is build-independent.

## Unresolved items / next steps

1. Decide the fix policy for piece-boundary silence stacking (trim pads
   vs. fewer pieces) — culprit 1.
2. Decide the speech-side em-dash/hyphen normalization policy —
   culprit 2. (My replies habitually use em-dashes; converting them
   speech-side to a short break would remove the most audible offender
   without changing chat text.)
3. Latency-before-speech remains the open voice-latency workstream —
   culprit 3.
4. No acceptance claim: audibility of any fix requires Alex's ears, not
   offline waveform math.

## Readiness effect

No capability, code path, or setting changed. The pause mechanisms are
now precisely located in three owners (worker splitting × engine pads;
phonemizer break tokens; pre-speech latency), ready for a scoped repair
decision.
