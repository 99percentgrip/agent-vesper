# Multi-turn playback pipe failure: cause, repair, and device-pending retest

> **Scope:** VRO-17 focused continuation of CPU production acceptance. Alex
> reported real-device speech working on the first two attempts, then failing
> to finish sentences from the third attempt onward, with repeated
> `speech failed: playback failed: player pipe write failed (device closed?)`
> under BOTH CPU and Automatic policies. This report records the FAILED
> device result, establishes the failing path and its actual layers, repairs
> what the evidence supports, and prepares a targeted retest. It is not PR-5,
> not an NPU claim, and not a transcription-model change.

## Device acceptance status (recorded first)

- **Repeated-turn playback/continuity on the real device: FAILED** (Alex's
  evidence: turns 1–2 completed; from ~turn 3 onward sentences stopped
  finishing, with repeated playback-failure warnings, under both CPU and
  Automatic).
- Preserved with their actual scope: the earlier successful turns, the
  qualitative "way faster" improvement, all software-only loop receipts
  (PTY passes, policy suites, timing), and the earlier single-turn
  acceptance passes. None of those covered sustained multi-turn real-device
  playback.
- The accelerator registry is empty (`registered_routes()` returns none);
  both CPU and Automatic resolved to the **CPU** route. No NPU factory,
  inference, or offload call exists or was claimed.

## §1 The failing path and its actual layers

`src/voice_playback.rs::push_pcm` produced the exact warning text at its
`write_failure_reason` fallback. Traced end to end:

- Host (`main.rs`) → `ConversationHost::speak_unit` → `SpeechWorker`
  (one worker per TUI process; one playback lane; per-segment streams)
  → `PlaybackOwner` (one `aplay` child per speech stream over stdin).
- Preview shares the same `PlaybackOwner` shape (Settings pack screen
  worker) — the same write path, its own stream lifetime.

Controlled experiments established the **layer separation** the old wrapper
text conflated:

1. **EPIPE at an ordinary pipe write means no reader remains on the pipe.**
   At an ordinary stdin write, `EPIPE` means no read-end descriptors remain
   open — in this architecture that is the child having exited (a reaped
   child's next write fails `BrokenPipeError`; an un-reaped SIGTERM'd child
   still accepts writes into the 64 KiB pipe buffer — measured: both). So
   `pipe write failed` established *the player process was gone before the
   write*, not device disconnection. This is a distinct layer from an ALSA
   PCM underrun error reported by a still-running player: that is stderr
   text classified separately, never a pipe errno. (Correction of an
   earlier wording: EPIPE itself does not prove *reaping* specifically —
   only that no reader remains; the reap distinction was established by
   the paired experiment, not by the errno alone.) "(device closed?)" was
   an invented hypothesis.
2. **The real installed player (`aplay` 1.2.16) with the production argv
   ran `--fatal-errors`, whose man page states it "aborts immediately" on
   any error, e.g. xrun, instead of recovering.** Under per-piece feeding
   (first piece ≈1.5 s PCM; successor synthesis gaps 1.6–2.5 s), a
   device-side underrun on resume makes aplay EXIT mid-stream — the next
   piece's write then hits a dead pipe. The null-device/fast-sink fixtures
   never xrun, which is exactly why every prior automated test passed while
   the real device failed.
3. **The visible storm was per-segment, not per-piece.** In-segment
   short-circuiting was already correct (the `failed` flag stops remaining
   pieces; one terminal outcome per segment). Each hygiene sentence is its
   own segment/stream, so a wedged player yields one warning per sentence —
   matching the repeated screenshot lines.
4. **Ruled out with evidence:** stale-build mismatch (the candidate's argv
   contained the flag), NPU involvement (empty registry), worker
   replacement on Settings refresh (no-op reload fixed earlier; no
   `Replace` sent), capture/player ownership overlap (separate owners),
   end-of-piece premature EOF (gaps do not close the stream — pinned), and
   the historical Stop-latch (next-turn latch clearing pinned green).

## §2 Diagnostics (bounded, sanitized)

- `push_pcm` now preserves the ORIGINAL `io::Error` (kind + raw OS code) in
  the message before any cleanup: e.g.
  `pipe write failed (os error 32); the player process is no longer reading`,
  optionally refined by the player's own bounded stderr classification
  (new classes: xrun/overrun/underrun, input/output error).
- Player lifecycle evidence retained: creation (spawn args validated),
  stdin close/EOF ownership marker, exit code via bounded `try_wait`, the
  stderr collector's classification (4096-byte cap, concurrent drain — no
  backpressure), and a child-exited-before-close marker for short
  consumers. No raw stderr, transcripts, audio, or credentials are stored.
- The dead-stream failure settles once per stream: the owner drops the dead
  child immediately (no further pieces write into the gone pipe) and the
  next turn opens a fresh stream.

## §3/§4 Red→green repair (smallest evidenced corrections)

New suite `apps/agent-vesper-tui/tests/voice_multiturn_playback.rs`
(11 tests; controlled players at realistic bounded rates; no audio device):

**Red receipts on the pre-fix tree (verbatim):**

```text
pipe_write_failure_reports_the_original_error_not_an_invented_cause ... FAILED
  'the invented device-closed hypothesis must be gone:
   player pipe write failed (device closed?)'
production_argv_does_not_request_fatal_error_behavior ... FAILED
  'the player argv must let the player recover device errors (xruns)
   by its documented default; --fatal-errors aborts the stream mid-turn'
short_consumer_cannot_yield_a_drained_receipt ... FAILED
dead_player_fails_segment_once_and_next_turn_recovers ... FAILED (turn-1 drain
  aborted by the --fatal-errors-era argv semantics under the aborting player)
```

**Corrections (all in the owning boundary, `voice_playback.rs`):**

1. **Removed `--fatal-errors` from the production argv.** aplay's
   documented default *recovers* xruns; the flag converted every
   recoverable device error into a mid-stream child death — the
   established mechanism behind Alex's turn-3+ failures.
2. **Original-error preservation** (`write_failure_reason`): the message
   now carries the actual io kind/OS code and the truthful consequence
   ("the player process is no longer reading"), never an invented cause.
3. **Dead-stream containment:** a failed write now takes the dead child out
   of the stream state and marks the stream stopped — remaining pieces of
   that segment fail fast instead of writing into a gone pipe, and the next
   turn's `begin_stream` opens a FRESH child (latch-clearing preserved).
4. **Short-consumer honesty:** exit 0 certifies `Drained` only when the
   child exited AFTER our stdin EOF; a child observed dead BEFORE the
   close (short consumption) reports a truthful failure, never completion.
5. **Appended-stream byte accounting:** `begin_stream` on an open stream no
   longer resets the cumulative byte count (append keeps counting; only a
   new child resets).

**Green receipts (post-fix, same suite):** `11 passed; 0 failed` — including
ten consecutive turns in ONE process through the real worker (all spoke),
turn-3 player death failing exactly once with turns 4–6 recovering on the
same worker, inter-piece gaps never ending a stream, delayed first read
accepted, early-success/short-consumer/nonzero exits all truthful, and the
EPIPE-mechanism pin (write-after-reap fails; write-after-SIGTERM-before-reap
succeeds).

## §5 Transcription findings (separate scope, kept open)

The screenshot's garbled transcript is recorded as an **open observation**,
not a diagnosed defect:

- STT route: shared `SidecarStt` → `voice_transcribe.py` → faster-whisper
  `base`, CPU, int8 (env `AGENT_VESPER_WHISPER_MODEL`, unset ⇒ `base`).
- `vad_filter=True` on every 30 s chunk (silence/trailing-silence
  hallucination guard; pinned by the existing fixture asserting the kwarg).
- **Language policy:** no `language=`/`task=` argument is forced, so
  faster-whisper auto-detects per chunk; the sidecar descriptor's `en` is
  metadata only. Plausible contributors to garbling — model size/quant,
  microphone quality, cross-chunk language switching — are hypotheses; none
  proven, and no STT change was made in this unit. A user-facing language
  control remains a separately recorded small-scope decision.
- No reasoning modes, permissions, tool policies, or message history were
  touched; tests used controlled no-tool replies.

## §6 Verification on the final optimized application

```text
candidate: target/voice-candidates/agent-vesper-tui-multiturn-repair
build:     cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui --release
sha256:    7bd31168b25817b495406581982dad0f83c787cbb2dc4cb7ab690c374a6fce6a
commit:    8f258ba28f4ea2f749526fb32b5b180ecc7dcead (dirty workspace; this unit's files listed below)
prior candidate preserved: agent-vesper-tui-voice-cpu-acceptance (d633b4c3…) untouched
```

| Gate | Result |
|---|---|
| voice_multiturn_playback (new, 11 tests) | **passed** (red receipts above) |
| TUI voice lib / default lib (R9 parity) | **passed** (278 / 242) |
| r3_speech_worker / pipeline / playback_diagnostics / interruption_lifecycle / policy_parity | **passed** (6 / 1 / 2 / 5 / 7) |
| pr4_f9_gate / pr4_wiring / r3_voice_pack / voice_execution_policy | **passed** (10 / 10 / 10 / 10) |
| r3_loop_pty.py direct / VRO / Preview (repair candidate) | **passed** |
| clippy `-D warnings` (voice-kokoro) | **passed** |
| architecture / naming-guard / acceptance | **passed** (28 / 33 frozen / 23) |
| rustfmt (changed files) | **passed** |
| workspace fmt | not fully green — pre-existing `ui.rs` lines only |
| MSRV / cross-target / CI / cargo-deny | **not run** (as before) |

Incidental unrelated fix: `r3_voice_pack::pack_readiness_reports_not_installed_on_empty_root`
asserted the REAL pack root without redirecting `XDG_DATA_HOME` (it only
passed by racing another test's env guard under parallel threads); it now
redirects first — green both single- and multi-threaded.

Storage: all fixtures under owned temp roots; 0 residue after cleanup; the
installed pack (118 009 413 B) untouched; disk 262.6 GiB free; no downloads,
installs, or global scans. Software delivery (PCM counts, drains) is
recorded separately from audible completeness — a null/controlled sink
certifies nothing about real speakers.

## Files changed by this unit

- `apps/agent-vesper-tui/src/voice_playback.rs` — argv repair, errno
  preservation, dead-stream containment, short-consumer honesty, append
  accounting, xrun/IO-error stderr classes
- `apps/agent-vesper-tui/tests/voice_multiturn_playback.rs` (new, 11 tests)
- `apps/agent-vesper-tui/tests/r3_voice_pack.rs` — env-redirect isolation fix
- `apps/agent-vesper-tui/Cargo.toml` — test target entry
- Docs: this report; `voice-user-latency-acceptance.md` (FAILED status);
  `evidence-index.md`; `migration-status.md`; app/tests AGENTS.md entries.

## Residual evidence gap (honest)

The reproduction used controlled players (including one that dies mid-stream
exactly like an xrun-aborting aplay) plus the real aplay against the null
device with the production streaming pattern (starvation up to 5 s — no
abort without the flag). The **real-device xrun under load** cannot be
triggered from this environment without playing audio. The causal chain
(flag forces abort on recoverable error → child dies mid-stream → next
piece's write EPIPEs → repeated warnings) is established at every layer
that does not require sound; the final confirmation is Alex's retest.

## Targeted retest (short, one sitting, on the new candidate)

```sh
cd /home/Alex/Projects/agent-vesper
./target/voice-candidates/agent-vesper-tui-multiturn-repair
```

1. F9 and let the reply finish; then **four more F9 turns in a row** —
   every turn should speak to completion (turns 1–2 alone do NOT close this).
2. If any turn fails: note the exact new message (it will now carry
   `os error N` and, when the player says so, an xrun/IO classification) —
   that single line now identifies the actual layer.

Status until then: **repeated-turn real-device playback/continuity: FAILED —
PENDING RETEST on the repair candidate.** No NPU performance claim from
Automatic (empty registry; CPU route), no full voice acceptance, no PR-5.
