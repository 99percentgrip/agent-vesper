# VRO-17 NPU voice — real-device user acceptance (2026-09-23)

Record created: 2026-09-23 10:21 AM PST (2026-09-23 18:21 UTC−8→+8 next day
note: the machine clock is PST; the listening test itself occurred earlier
the same day — exact test start time not captured; bounded evidence below
places it between ~09:46 and ~10:10 PST).

**Status: positive real-device user acceptance for the tested NPU-enabled
conversation experience and perceived speech continuity/naturalness.**

> **Current tested conversation: accepted by Alex for current use. Speech
> continuity and naturalness: user-confirmed on the tested setup. Technical
> execution evidence retains its documented scope. Full VRO-17 and release
> acceptance remain separately gated.**

## User evidence (verbatim)

Alex reported:

> Cooooollll all working with npu and there is not gaps its sounds natural

Alex subsequently authorized this documentation-only closeout.

Supported reading of the statement, no more:

- The tested run used the NPU recognition route, per Alex's own words
  ("working with npu").
- The previously reported disruptive inter-sentence gaps are **no longer
  apparent to Alex in this run**; speech sounds natural to him.
- This is genuine listening evidence. It is **not** a measured zero-gap
  result, not a claim that ordinary punctuation pauses are removed, not a
  claim of universal gap-free operation, and not a per-stage hardware
  measurement. "No gaps" in the user's words records perceived continuity.

This is a **new current success**, superseding the earlier verdict recorded
in [`voice-continuity-listening-acceptance.md`](voice-continuity-listening-acceptance.md)
("there is still a delay but it is much better then before and i think this
is good enough for now"). That earlier result remains history at its own
scope and date; it is not carried forward as a present complaint. The
historical measured ~5.2 s first-large-successor cold-start gap likewise
remains a historical measurement on the phoneme-repair candidate; no matched
new measurement supersedes it, and none is claimed here.

## Tested artifact identity

**User acceptance confirmed; exact tested artifact association pending.**

What the evidence establishes (read-only inspection, 2026-09-23 10:21 PST):

- Both live TUI processes (PID 924964 since Sep 22 00:14; PID 1051553 since
  Sep 23 05:18, `--resume 0bf4fa51…`) execute the installed
  `~/.local/share/agent-vesper/agent-vesper-tui`, SHA-256
  `c4f29171eaad90cefc7e01af6f12f5a3cc47031bc71b558606243dcd0e07a11f`
  (23,015,672 bytes, Sep 18 build) — which contains **no FLM feature at
  all** (`strings` audit: zero `whisper-v3`/`flm` markers). Neither live
  session can be the NPU voice test.
- Two repository session files record the apparent listening-test turns
  (`.agent-vesper/sessions/3a79c883…json` updated 10:03:39, and
  `62b3a3ed…json` updated 10:09:42, both cwd = repo root): short
  one-sentence and few-sentence no-tool replies — the established
  voice-test prompts. Their host process has exited.
- `~/.bash_history` (last flushed 08:00) predates the test window and shows
  no post-08:47 launch; konsole retains no command history. No process,
  shell, or journal record identifies which binary served those sessions.
- Root `.agent-vesper/config.toml` was rewritten at 10:08:21 — between the
  two recorded session updates — with `[voice] enabled=true,
  tts="voice-kokoro", voice="af_heart", stt_compute="cpu",
  tts_compute="cpu"`. Current settings after the test are **not** a
  historical record of the in-test selections; per-process NPU verification
  plus a session draft selection would not necessarily persist to this
  file. No inference is drawn from it about the tested route.

Candidate binaries present (all preserved, none installed, none modified):

| Candidate | SHA-256 (first 16) | Bytes | Features evidence |
|---|---|---|---|
| `agent-vesper-tui-verify-read-repair` | `b289a6c1537ecaad…` | 22,670,024 | voice-flm + voice-kokoro (current delivery) |
| `agent-vesper-tui-flm-npu-stt-candidate` | `3c634c739e773d2c…` | 22,652,104 | voice-flm + voice-kokoro (2026-09-22) |
| `agent-vesper-tui-continuity-boundary-repair` | `fa0c371305fc95bc…` | 22,648,008 | voice-kokoro; FLM code present |
| `agent-vesper-tui-continuity-phoneme-repair` | `9d59189a00999fff…` | 22,393,288 | voice-kokoro (accepted CPU listening baseline, Sep 21) |

Alex explicitly rejected attaching this acceptance to the
`agent-vesper-tui-verify-read-repair` basename by default. No basename,
checksum, or prior candidate is claimed as the tested artifact. A binary
hash would identify bytes only — it would not prove source equality,
reproducibility, or correspondence to today's modified workspace; none of
those is claimed for any candidate here.

**Missing evidence:** the launch command (or a live process/exec identity)
for the exited session that served the 10:03/10:09 listening turns. One
precise identity question is asked alongside this record; the acceptance
itself does not wait on the answer.

## Accepted configuration baseline (scoped)

The accepted experience is the composition documented by the current
implementation records, attributed to their evidence — not measured by the
user's sentence:

- Shared local **CPU Silero VAD** preprocessing
  (`voice-npu-stt-implementation-progress.md`).
- **FLM/NPU speech recognition** through the owned loopback service
  (`voice-flm.rs`; placement evidence = process/device/model correlation —
  no per-request offload receipt exists; no "full-NPU" claim).
- **Reasoning provider separately configured** (session records; not part
  of the voice composition).
- **Kokoro speech synthesis on CPU** (`voice-cpu-production-acceptance.md`
  and pipeline records; NPU TTS is unestablished and Alex's words do not
  change that).
- Continuity behavior: bounded first piece + sentence-level successors
  (`voice-continuity-phoneme-repair.md`) with the two-unit banked handoff
  (`voice-continuity-boundary-repair.md`).

This documented tested configuration is the **user-accepted reference for
future voice regression comparisons**. Its executable identity remains
unresolved and must not be inferred from any candidate currently on disk; if
later established by evidence, that identity may be appended without changing
the bounded acceptance. The older accepted CPU candidate (`9d59189a…`) remains
preserved separately as rollback evidence. No rebuild, rename, copy, overwrite,
install, strip, patch, deletion, tag, commit, push, or release was performed for
this baseline; nothing was installed merely because it was accepted.

## Scope against the PRD's requirement items

- **R5 continuity/listening** — the listening requirement now has positive
  current user evidence for perceived continuity and naturalness on the
  tested setup. Quantitative latency/accuracy coverage remains separately
  open; "no gaps" is not converted into a measured silence metric.
- **TUI device-acceptance scope** — real-device voice conversation
  (microphone recognition + spoken reply) is user-accepted for current use.
  Not exercised or claimed: repeated-interruption stress, error/cleanup
  cases, multi-hour stability, or non-conversation surfaces.
- **R16 recognition evidence** — unchanged in strength: real-model
  no-device receipts, PTY F9 round-trips, and Verify reliability repair
  (see `voice-verify-read-failure-repair.md`). The user's runtime
  confirmation is consistent with, and does not exceed, that scope.
  **R16 is not globally complete**; NPU TTS remains open/unestablished.

## Still open (unchanged by this record)

Quantitative latency/accuracy coverage; repeated-interruption and
error/cleanup device cases; placement evidence limits (correlation, not
per-operator offload); NPU TTS; default-build capture-storage coverage gap
(R20); MSRV/cross-target/CI; PR-5 and release work; the ~745 MB partial
Llama residue decision and the vendor fallback-download question. Existing
test receipts remain valid at their recorded scope; nothing was reset to
unknown, and no historical test was promoted to a fresh run.

## Checks performed for this documentation-only update

Relative links/anchors within this record verified against the files named;
Markdown whitespace reviewed; git diff reviewed to attribute this unit's
changes (documentation only). No program suites, builds, inference,
device probes, Verify actions, FLM launches, microphones, or speakers were
run; earlier receipts are referenced, not rerun.
