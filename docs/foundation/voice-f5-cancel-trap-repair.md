# Voice F5-cancel trap repair (v0.22.6 regression found in the field)

Date: 2026-09-13. Status: **repaired and verified; pending Alex's real-device confirmation**.
Scope: `apps/agent-vesper-tui/src/voice.rs`, `apps/agent-vesper-tui/src/main.rs` + tests.
Discovered by Alex on his machine after upgrading to v0.22.6 ("push to talk
returns 'Voice preparation cancelled' — not recording at all").

## Root cause (traced, not guessed)

Alex's real voice environment: `~/.local/share/agent-vesper/voice-venv/`
exists but **lacks `faster_whisper`** (`ModuleNotFoundError` reproduced
directly on his machine); system `python3` also lacks it; `uv` is not on
PATH but the **bundled** `~/.local/share/agent-vesper/uv` exists and the
install path itself was proven working (test venv + `uv pip install
faster-whisper` succeeded in ~minutes on this machine).

Failure chain on first F5 press:

1. `start()` → `prepare()` → no interpreter has `faster_whisper` →
   bootstrap: `uv venv` + `uv pip install faster-whisper` (multi-minute,
   silent, network-dependent).
2. During that window the phase is `Preparing`; the old `toggle()` mapped
   **F5 during Preparing to cancel**. F5 is also the voice toggle key, so
   the natural "press again" reflex — or any stray F5/repeat — set the
   cancel flag, aborted the one-time install mid-flight, and surfaced
   `Voice preparation cancelled. F5 Retry / Del Discard.`
3. Retry re-entered the same window; nothing was ever recorded.

Contributing design flaw: the error hint told the user to press F5
(Retry) — the very key that cancels a retrying preparation.

## The fix

| Change | Before | After |
|---|---|---|
| F5 during `Preparing` | set cancel flag (killed install) | **no-op** — F5 is the voice toggle; the in-flight preparation continues to completion |
| `Del` during `Preparing`/`Transcribing` | only worked in `Error` | explicit `cancel_work()` cancel (audio retained) |
| Phase hints | `F5 Cancel` / `F5 cancels` during Preparing/model-load/progress | `Del cancels`; Preparing hint explains the multi-minute first use |
| `toggle()` Preparing hint | `Preparing voice… F5 cancels.` | `Preparing voice… this can take several minutes on first use; Del cancels.` |

F5-cancel during **Transcribing** is preserved (a >90-second transcription
legitimately needs an interrupt; the PTY suite covers it).

`Transcribing` keeps F5-cancel because that phase follows a successful
recording; `Preparing` is the phase where nothing is captured yet and the
install must not be killable by the feature's own key.

## Red-first evidence

On the pre-fix tree (tests appended, buggy `toggle()` retained), verbatim:

```
thread 'voice::tests::f5_during_preparing_is_ignored_not_a_cancel' panicked:
F5 during Preparing must not set the cancel flag
test voice::tests::f5_during_preparing_is_ignored_not_a_cancel ... FAILED
```

On the fixed tree the same test passes, plus a second test proving
`cancel_work` cancels in Preparing/Transcribing and stays inert in
Idle/Recording/Error.

## Verification

- `cargo test -p agent-vesper-tui --bin agent-vesper-tui voice`: **7/7**
  (2 new + 5 preserved lifecycle tests).
- `cargo test -p agent-vesper-tui --lib`: 241/241.
- Real end-to-end PTY through the production binary
  (`voice_pty.py` + fixture recorder/model): **PASS** — mouse/F5 lifecycle,
  10-minute PCM, >90-second progressing transcription, composer editing,
  retry/discard, early exit, disk failure, shutdown cleanup
  (`/tmp/voice-pty-fix.log`). The PTY suite exercises F5-cancel during
  Transcribing and still passes, proving the preserved path.
- `voice_chunks.py` (real numpy): PASS.
- Workspace: **2,326 / 0 failed** · acceptance **23/23** · clippy `-D
  warnings` clean · fmt clean · naming-guard 18 frozen.

## Environment note for Alex (not a code change)

His voice venv now still lacks `faster_whisper`; with this fix the first
F5 will run the one-time install to completion without being cancellable
by F5 (Del cancels explicitly). Progress is visible via the phase line.
He can also pre-build it:

```
~/.local/share/agent-vesper/uv venv ~/.local/share/agent-vesper/voice-venv
~/.local/share/agent-vesper/uv pip install faster-whisper \
  --python ~/.local/share/agent-vesper/voice-venv/bin/python
```

## Real-device acceptance (user-supplied, 2026-09-14)

After updating and testing on his Linux machine, Alex confirmed push-to-talk
works end to end: a >60-second dictation transcribed cleanly and coherently,
247 words total, nothing truncated, tail intact. This closes the real-device
acceptance that the v0.22.6 voice report explicitly left open (all prior
voice evidence was fixture-based, no physical microphone). macOS capture and
native OS-permission-denial paths remain unexercised by design (Linux-only
device test).

Observed, known-behavior note (not a regression of this repair): the
transcript's tail repeated "1 min" ~77 times — the classic
faster-whisper artifact on trailing silence/noise after the speech ends.
The v0.22.6 scope statement already disclaims perfect recognition
("fixed nonoverlapping slices… does not promise perfect speech
recognition"). A silence-trim pass before transcription is a possible
follow-up if it annoys in practice.

## Deviations / open items

- The deeper UX question — preparation should stream progress instead of
  a static hint — is real but larger; this repair fixes the trap with
  minimal blast radius.
- Real-microphone acceptance on Alex's device is his to confirm after
  installing a build with this fix (0.22.6 installed build predates it;
  not yet released).

## Readiness effect

First-use voice on Linux no longer dead-ends in
"Voice preparation cancelled": F5 starts preparation and keeps it running;
Del is the documented, explicit cancel; retry after a real failure no
longer instructs the user to press the key that cancels their retry.
