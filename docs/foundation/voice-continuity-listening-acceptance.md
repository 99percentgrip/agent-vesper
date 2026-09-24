# VRO-17 continuity and list-formatting listening acceptance

Date: 2026-03-03

Status: **PASSED for the reviewed continuity/list-marker repair; residual cold-start delay remains open**

## Objective

Record Alex's device-level verdict on the preserved VRO-17 continuity and
formatting-repair candidate without broadening the repair scope or overstating
what listening can prove.

## Method and artifact

Alex ran the preserved candidate from the repository:

```sh
cd /home/Alex/Projects/agent-vesper
./target/voice-candidates/agent-vesper-tui-continuity-phoneme-repair
```

Candidate identity recorded by the implementation report:

```text
agent-vesper-tui-continuity-phoneme-repair
sha256 9d59189a…
```

The listening checklist covered an ordinary short reply, a longer reply with
numbered steps/headings, and interruption followed by another request.

## Exact device evidence

Alex's verdict, verbatim:

> yeah i mean there is still a delay but it is much better then before and i think this is good enough for now

This is positive comparative listening acceptance: continuity is materially
better than before and is good enough for the current repair. It is not a
claim that delay is eliminated, acoustics are objectively certified, or every
voice requirement is complete.

## Implemented behavior accepted in this unit

- Replaced routine approximately 28/48-character successor subdivision with a
  bounded early first piece followed by sentence-level successors or the
  remaining approved sentence.
- Preserved prompt first speech, one worker, one model-session owner,
  one-prepared-unit lookahead, ordering, cancellation, playback ownership, and
  existing model/token/budget fallback.
- Kept recognized streamed list prefixes with their following item so a marker
  such as `1.` is not sent to Kokoro as an empty-phoneme request.
- Preserved meaningful numeric and symbol content on the truthful synthesis
  path; unexpected zero-phoneme results for meaningful input remain visible
  failures.

## Root cause and regression-first evidence

Two independent causes were established before repair:

1. Routine micro-subdivision introduced model fade quiet at many artificial
   mid-sentence boundaries. The old policy measured about 12 pieces, 11
   artificial mid-sentence insertions, and approximately 7.2 s stacked quiet
   on the representative passage.
2. Streaming hygiene could finalize a standalone numbered-list marker such as
   `1.`. Kokoro produced zero usable phoneme IDs for `1.` alone, while `1. Do`
   remained pronounceable; therefore global numeric deletion would have lost
   meaning.

The regressions failed on the pre-repair behavior and passed after the repair:

```text
hygiene_formatting_units: 13 passed
voice pipeline subdivision fixture: old 28/48 policy rejected; sentence-level successor policy accepted
PTY production path with word-sized streamed deltas: standalone `1.` request reproduced before repair; absent after repair
```

Mandatory distinctions remain pinned: headings retain their words; `The answer
is 1.`, decimals, and versions retain content; symbols used as content still
emit; meaningful zero-phoneme failures are not globally suppressed.

## Measured tradeoff

The repaired representative path measured:

```text
pieces: 12 -> 5
stacked fade-quiet: approximately 7.2 s -> approximately 2.9 s
artificial mid-sentence insertions: 11 -> 1
aggregate RTF: approximately 0.66 -> approximately 0.66
candidate onset comparison: approximately 2.67 s vs 2.73 s
```

The continuity gain intentionally accepts larger sentence-level successor
units after the bounded first piece. A paced sink remained gap-free except for
one quantified approximately 5.2 s cold-start gap at the first large successor.
No prebuffering, parallel inference, extra model session, larger lookahead,
thread increase, waveform trimming, inserted silence, or audio post-processing
was introduced.

## Commands and verification receipts

The implementation report records these current-tree gates as green:

```text
hygiene_formatting_units: 13 passed
four isolated PTY modes: passed
Clippy: 3 feature sets passed with warnings denied
cargo xtask architecture: 30 packages passed
cargo xtask naming-guard: 36 frozen checks passed
cargo xtask acceptance: 23 passed
scoped rustfmt check: passed
```

This acceptance-only documentation update reviewed links and whitespace; per
repository policy it did not rerun program test suites or a release pipeline.

## Deviations and unresolved items

- Residual delay remains audible and explicitly open; Alex accepted it as good
  enough for now, not as eliminated.
- The approximately 5.2 s first-large-successor cold-start gap remains a
  measured follow-up. A bounded audio-duration prebuffer is only a proposal and
  was not slipped into this repair.
- The pre-existing decimal-at-stream-chunk-boundary limitation remains open.
- STT garbling, NPU STT/TTS, cloud commitments, PR-5, cross-target/MSRV/CI, and
  release work remain outside this acceptance.
- No installer was run and no installed binary was replaced.

## Files and readiness effect

This closeout updates the listening acceptance record, evidence index, owning
PRD, migration status, and nearest DOX status text. It changes no production
code or artifact. The reviewed continuity/list-marker repair is accepted for
current use; VRO-17 remains open for the explicitly listed residual and
out-of-scope work.
