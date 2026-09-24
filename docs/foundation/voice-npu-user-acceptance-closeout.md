# VRO-17 NPU voice user-acceptance closeout

Date: 2026-09-24
Work unit: documentation-only acceptance closeout

## Objective

Close the documentation record for Alex's successful native voice test without
changing production code, replacing a candidate, changing Settings, interrupting
running processes, or performing release/PR-5 work.

The owning acceptance record is
[`voice-npu-user-acceptance.md`](voice-npu-user-acceptance.md). This execution
report records the closeout method and its bounded readiness effect; it does not
create a second or broader acceptance claim.

## User evidence

Alex's verdict is preserved verbatim:

> Cooooollll all working with npu and there is not gaps its sounds natural

Bounded interpretation: the tested conversation is accepted for current use,
and continuity and naturalness were user-confirmed on the tested setup. This is
listening evidence, not a measured zero-millisecond-silence result. It does not
claim elimination of ordinary punctuation pauses, universal gap-free behavior,
recognition accuracy, interruption coverage, or full-pipeline/per-operator NPU
placement.

## Methods and commands

This closeout used documentation and repository-state inspection only:

```text
git status --short
git diff --check -- docs/voice-oracle-extraction-prd.md \
  docs/foundation/AGENTS.md \
  docs/foundation/evidence-index.md \
  docs/foundation/voice-npu-user-acceptance.md \
  docs/foundation/voice-npu-user-acceptance-closeout.md
python3 <bounded relative-Markdown-link checker for the five files above>
```

No program test suite, build, inference run, microphone/speaker probe, FLM
launch, installer, Settings mutation, process interruption, commit, push, tag,
or release command belongs to this closeout. Earlier test/build activity and
working-tree implementation changes are not acceptance-closeout evidence.

## Files

- `docs/foundation/voice-npu-user-acceptance.md` — authoritative acceptance,
  tested-artifact uncertainty, bounded configuration baseline, historical
  context, caveats, and open gates.
- `docs/foundation/voice-npu-user-acceptance-closeout.md` — this formal
  execution report.
- `docs/foundation/evidence-index.md` — evidence-ledger link and bounded summary.
- `docs/voice-oracle-extraction-prd.md` — owning PRD link to the acceptance.
- `docs/foundation/AGENTS.md` — nearest DOX ownership entry.

## Exact evidence and receipts

### Acceptance receipt

Verbatim human observation:

```text
Cooooollll all working with npu and there is not gaps its sounds natural
```

The acceptance record preserves the historical earlier bounded verdict
("there is still a delay but it is much better then before and i think this is
good enough for now") and the historical failure/repair records rather than
rewriting them as though the latest result had always held.

### Tested executable identity

**Unresolved.** Available evidence does not identify which executable served the
successful test. In particular, this acceptance is not attached to:

- `agent-vesper-tui-verify-read-repair` or checksum prefix `b289a6c1…`;
- `agent-vesper-tui-continuity-boundary-repair`;
- the Sep-18 installed binary with digest prefix `c4f29171…`; or
- the later session-built candidate with SHA-256
  `8895a711b4cb19c4f37eebc7b4d57d02e12f334e796f5f91d577c91d0d7e6398`.

The `c4f29171…` live-process observation belongs to the earlier Verify
investigation, and the `8895a711…` candidate was built after Alex's test. Neither
can retroactively identify the successful executable.

### Regression baseline

The accepted configuration baseline for future comparison is bounded to:

- shared CPU Silero VAD;
- selected and verified FLM/NPU speech recognition, with placement evidence no
  stronger than the existing process/device/model correlation;
- a separately configured reasoning provider; and
- CPU Kokoro speech synthesis with the documented continuity pipeline.

The configuration is the regression comparator even while executable identity
remains unresolved. A later evidence-backed executable identification may be
appended; no candidate may be inferred or substituted.

### Documentation verification receipt

The bounded Markdown link check reported:

```text
LINK_CHECK files=5 local_targets=181 missing=0
```

The whitespace/error check reported:

```text
DIFF_CHECK exit=0
```

Per the user's prose-only preference, program and release suites were
intentionally not run.

## Deviations

The preceding session drifted outside the documentation-only authorization: it
ran broad test/build/acceptance work, created a candidate, edited harnesses, and
began implementation work around intermittent FLM backend exits. Those actions
are not represented as authorized by this closeout, are not used to identify
Alex's tested executable, and are not acceptance evidence for this work unit.
This closeout did not attempt to approve, revert, finish, release, or install
those unrelated working-tree changes.

## Unresolved items

- Exact executable identity for Alex's successful test remains unresolved.
- Quantitative continuity/latency and recognition-accuracy measurements were not
  part of the user verdict.
- Repeated interruption, error/cleanup, and broader device-matrix coverage are
  not established by this acceptance.
- Full VRO-17 completion and public-release acceptance remain separately gated.
- NPU TTS and full-pipeline/per-operator NPU placement are not claimed.

## Readiness effect

The tested native conversation experience is accepted for current use at the
bounded configuration and listening scope above. This supplies a durable future
regression baseline and supersedes the earlier continuity complaint as the
current user verdict while preserving that earlier evidence historically. It
does not close full VRO-17, PR-5, release, quantitative, interruption, accuracy,
or hardware-placement gates.
