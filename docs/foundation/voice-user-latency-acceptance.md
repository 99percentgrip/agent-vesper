# Voice latency user acceptance — initial retest

## Objective and status

Record Alex's manual retests of the repaired recording, Preview and spoken-response paths before VRO-17 is described as implemented or accepted.

**Current device verdict: continuity/formatting repairs implemented and software-verified; Alex's listening comparison PENDING.** The [continuity/phoneme repair](voice-continuity-phoneme-repair.md) replaced the 28/48 micro-cut subdivision (clause-sized onset piece + sentence-level successors; stacked fade-silence 7.2 s → 2.9 s in the fixed passage) and fixed the exact `"1."` formatting-unit phonemization failure through the full production path, with meaning-preservation distinctions pinned by tests. One residual quantified cold-start gap (~5.2 s at the first sentence-level successor) is documented with a separately-proposed bounded prebuffer awaiting review. Historical scope preserved below: the earlier build's ~20–40 s onset and ~10–15 s Preview delays; the latency repair's **"ok its way faster then before"**; the multi-turn repair restoring speech after the repeated `player pipe write failed` failures; and the continuity/phoneme device feedback that motivated this repair.

## User evidence

Alex reported, verbatim:

> So the finish, the first test was recording, I mean when it's first spoken it's 20 seconds. The recording is fast but I cannot tell you how much second, maybe 2-3 seconds delay. The second attempt is also fast but the first spoken was 40 seconds. And then... I did the third test which was again identical to test number one which was first spoken word after 20 seconds. The preview is also have a delay like 10 to 15 seconds.

Normalized without adding precision that was not measured:

| Attempt | Recording response | First spoken output | Result |
|---|---:|---:|---|
| 1 | approximately 2–3 s; described as fast | approximately 20 s | latency acceptance not met |
| 2 | described as fast; no numeric value | approximately 40 s | latency acceptance not met |
| 3 | reported identical to attempt 1 | approximately 20 s | latency acceptance not met |
| Preview | not applicable | approximately 10–15 s | latency acceptance not met |

These are human-observed approximate timings for the superseded tested build, not instrumented timestamps. The report does not infer whether those delays came from capture startup, transcription, provider/routing work, sentence gating, Kokoro initialization/inference, playback startup, or a combination.

Current candidate follow-up, verbatim:

> ok its way faster then before

This is positive device-level comparative evidence, but it contains no numeric or per-path breakdown.

## Methods and commands

- Reviewed Alex's manual observations against the current first-speech report, owning PRD and migration-status claim.
- No microphone, speaker, provider, installer, runtime, model, NPU workload or saved setting was operated by this documentation pass.
- No program test suite was run: this work unit only corrects acceptance records from new user evidence. Markdown/link/whitespace checks are recorded at closeout.

## Files

- Created `docs/foundation/voice-user-latency-acceptance.md`.
- Updated `docs/foundation/voice-first-speech-and-npu-assessment.md` to distinguish locally verified code from failed/pending user latency acceptance.
- Updated `docs/voice-oracle-extraction-prd.md` so the owning phase remains open and links this retest.
- Updated `docs/migration-status.md` to remove the contradictory device-acceptance-passed headline.
- Updated `docs/foundation/evidence-index.md` and `docs/foundation/AGENTS.md` to index this record.

## Required next acceptance

Do not mark the phase implemented or accepted until all applicable user-operated checks are rerun on the candidate build and Alex explicitly accepts the result. **Current candidate (this checklist): the release application verified by [CPU production acceptance](voice-cpu-production-acceptance.md), preserved at `target/voice-candidates/agent-vesper-tui-voice-cpu-acceptance` (sha256 d633b4c3…). Launch it from the repository root.** Settings in effect: Voice enabled, Natural Voice (Kokoro), am_michael, CPU compute for both stages (defaults).

1. Preview: cold first run and immediate warm repeat; record click-to-first-audible-word.
2. F9 capture: three turns, recording start responsiveness, Stop responsiveness, stop-to-visible-transcript and stop-to-first-audible-word.
3. Long response: confirm the first word starts promptly, the complete answer remains natural, and no long between-sentence gap returns.
4. Multi-turn/barge-in: interrupt spoken output, then confirm later turns still speak and no canceled output resumes.
5. Error path: preserve text and offer a truthful visible failure/retry path rather than silently losing speech or recording.
6. NPU STT and NPU TTS remain separate open implementation/acceptance gates; CPU behavior cannot be relabeled as NPU success.

The existing requirement already says the reported approximately 20-second delay is unacceptable. This record does not invent a new numeric product threshold. Final acceptance requires measured repeated results plus Alex's explicit approval.

## Exact evidence and readiness effect

Observed manual range:

```text
recording response: approximately 2–3 seconds on attempt 1; attempts 2/3 described as fast
first spoken output: approximately 20 s, 40 s, 20 s
Preview first spoken output: approximately 10–15 s
```

Readiness effect:

- The prior fixed-text local early-PCM benchmark remains valid for its narrow fixture scope.
- It does not predict or override this native user test.
- The earlier end-to-end latency acceptance failed; the new candidate has a qualitative comparative pass but no quantitative matrix.
- The latency repair can be reported as implemented and materially improved; VRO-17 cannot be marked fully accepted on the basis of current voice evidence.

## Deviations and unresolved items

- Timings are approximate and were not captured by an instrumented build.
- Cold/warm state, selected voice, exact text lengths, provider stage timing and acoustic onset were not independently captured.
- No diagnosis or production repair was attempted in this documentation-only work unit.
- Native NPU STT and TTS are still unimplemented and unaccepted.
- Full platform, CI, release and installation gates were not run.

## DOX and closeout

The nearest documentation and foundation contracts were re-read. The new evidence changes phase readiness and therefore updates the owning PRD, migration status, foundation evidence index and foundation ownership index. Root and `docs/AGENTS.md` remain unchanged because no repository-wide or documentation-boundary contract changed.

Closeout receipts:

```text
voice_relative_links_checked=22
voice_missing_relative_links=0
trailing_whitespace_issues=0
git diff --check: exit 0
```

The first broad link scan reported 43 apparent missing targets because URL-encoded spaces in pre-existing `Vesper%20bridge` links were not decoded. The corrected voice-scope checker decodes link paths and reports zero missing targets. No program tests were run because this was a prose-only acceptance correction with no source change.
