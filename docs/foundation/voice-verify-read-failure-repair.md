# VRO-17 R16 — Native Verify Read Failure: Diagnosis and Repair

**Work unit:** reliability repair of the FLM NPU Verify path after Alex's
user-tested failure (`Accelerated recognition · not verified … owned ASR read
failed or timed out`). Mission text 2026-09-23. Owning implementation record:
`voice-npu-stt-implementation-progress.md`; continuity context:
`voice-continuity-boundary-repair.md`.

## Objective

Record the user failure, reproduce it through the production Settings route,
establish the exact mechanism with evidence, apply the smallest correction in
the owning component (`apps/agent-vesper-tui/src/voice_flm.rs`), and prove the
fix through the native Verify application path — without a larger timeout, a
second supervisor, or any bypass.

## User failure (recorded, unattributed at intake)

Screenshot text: *Verification failed at the local boundary: voice provider
stt-flm-npu unavailable: owned ASR read failed or timed out. Accelerated
recognition stays unavailable; CPU recognition is unaffected.* Recorded as
**native NPU verification FAILED on the user-tested run; diagnosis pending**.
The screenshot established none of: which read failed, deadline expiry, or
why.

## 1. Failing-path and baseline establishment

- Exact string constructor: `voice_flm.rs::transcribe_request` read-loop
  `Err(_)` arm — the **only** producer of the user-visible phrase; confirmed
  by binary `strings` audit of both FLM-bearing candidates.
- The loop collapses every `Err` (timeout, ECONNRESET, EPIPE…) into one
  message; `REQUEST_DEADLINE` is 180 s, Verify's own bound is 240 s — so a
  deadline expiry could not match Alex's prompt failure.
- Executable identity on the live machine: both running TUIs execute the
  **installed** `~/.local/share/agent-vesper/agent-vesper-tui`
  (`c4f29171…`, mtime Sep 18) which contains **no FLM strings at all**;
  the failing Verify therefore ran in a *candidate-binary session* that has
  since exited. `.bash_history` shows the FLM and boundary-repair candidates
  were run directly. No build identity was assumed from the screenshot.
- Verify route traced: `settings_host.rs::run_flm_verify` → `FlmNpuStt::
  transcribe` → shared VAD (`voice_flm_vad`) → `FlmNpuStt::ensure_service`
  → `FlmProcess::spawn` (asset verify → port pick 18130–18139 → spawn →
  listener wait → log contract) → `transcribe_request`.

## 2. Read-only inspection (no kills during diagnosis)

Three `flm serve --asr 1 … --port 1813x` processes (22:56, 00:46, 00:55),
each PPID 1642 (user systemd — orphaned), ~330 MB RSS each, all holding
`/dev/accel/accel0`, listeners on 18130/18131/18132 — the entire first half
of the private port range. Launch shape exactly the owned contract; spawn
times inside this integration's prior test windows. No TUI process linked
FLM code. `/tmp/vesper-flm-child-stderr.log`: empty (the spawn path's file
redirect; `--quiet` produces no stderr).

## 3. Reproduction and correlated diagnosis receipt

**Production-route reproduction (before any cleanup):** the real PTY Verify
harness (`flm_f9_loop_pty.py`, Settings → Voice → Verify) against the
boundary-repair candidate reproduced the user string **verbatim**. Post-run
`ps`/`ss`: no new flm survived (our teardown worked); the three orphans
remained.

**Library reproduction:** `flm_stt_receipt` speech case failed in
**0.879 s** — too fast for any deadline; the read `Err` was an immediate
connection reset. Vendor-side probe (owned shape, port 18133) captured the
definitive receipt:

```
[FLM]  ASR mode enabled: reserving additional 1GB of memory
[FLM]  Using user-specified port: 18133
[FLM]  Loading model: ~/.config/flm/models/Whisper-V3-Turbo-NPU2
[ERROR] Failed to load ASR model: DRM_IOCTL_AMDXDNA_CREATE_HWCTX IOCTL failed (err=-22): Invalid argument
```

**Mechanism (established):** orphaned owned-shape ASR servers exhaust the
NPU's device contexts. A fresh server binds its listener fast, then model
load fails at `DRM_IOCTL_AMDXDNA_CREATE_HWCTX` (EINVAL); the child exits;
the in-flight request's read returns a reset (or EOF); the old read loop
labeled that "read failed or timed out". The mission's hypotheses were
separated explicitly: not a timeout (0.879 s), not asset corruption
(passive `assess_pack` = Installed throughout), not a broken install (the
same binary+model pass on a clean device), not a blocked pipe, not a
framing bug.

**Orphan disposability evidence** (per §2's rule): exact owned launch shape,
exclusive private port range, spawn times inside this work's test windows,
PPID 1642 with no live owner (both TUIs run an FLM-free binary). Terminated
by exact PID with TERM (all three exited; no KILL, no pkill, no by-port
action); ports and `/dev/accel/accel0` released. **Clean-device A/B:** the
same receipt binary then passed speech 4.134 s / warm 2.818 s. The
~745 MB partial Llama residue was not touched.

## 4. Corrections (owner-only, red→green)

### 4a. Stage-truthful read-failure classification

`transcribe_request`'s read loop now classifies `io::ErrorKind`:

- `ConnectionReset | ConnectionAborted | BrokenPipe` →
  **"owned ASR process exited while answering"** (the reproduced class)
- `WouldBlock | TimedOut` → **"owned ASR read timed out"**
- otherwise → the original bounded phrase

A `#[doc(hidden)]` test seam (`with_request_read_deadline_for_test`) keeps
the timeout case provable in bounded time; the production default remains
180 s (unchanged — no deadline inflation anywhere in this unit).

**Red proof:** with the classification reverted to the old collapsed arm,
`reset_connection_reports_server_death_not_read_timeout` and
`transport_error_classes_are_distinguished` both FAIL (0 passed, 2 failed);
restored, the suite passes (13/13, four consecutive parallel runs). The
double-server read loop was also repaired to terminate on the multipart
terminator (its old `len() > 200` heuristic raced the two-write request
under the new lock ordering) and the deadline-sensitive tests serialize on
the existing `FLM_STATE_LOCK` (the seam is process-global).

### 4b. Crash-leak prevention (durable child registry + reaper)

Why the leak existed: `FlmProcess::Drop` tears down correctly, but abrupt
host death (SIGKILL — the PTY harness does this at every test end) runs no
Drop, and a quiet flm child has no stdout write to trigger SIGPIPE.

- Every spawn records `{pid, port, host_pid, started_unix}` in
  `~/.local/share/agent-vesper/flm-child-registry/children/<pid>.json`;
  normal teardown clears the record.
- Before each spawn, the reaper scans the registry. A child is **abandoned
  exactly when its registrar host is gone** (`/proc/<host_pid>` absent) —
  never merely because PPid changed (systemd --user reparents to 1642, not
  1; the first PPid==1 draft was wrong and proved so live). Live-host
  children are never touched; the owned launch shape (`flm … serve --asr 1
  …`) is re-confirmed from `/proc/<pid>/cmdline` before any signal; PID
  reuse drops the record without signalling. Escalation is TERM → 3 s →
  KILL via the `kill` command, matching the existing Drop convention.
- **Pure std; no unsafe** (a PDEATHSIG `pre_exec` draft was discarded when
  the crate-level `forbid(unsafe_code)` refused the module-level allowance;
  the no-unsafe design is strictly better here anyway). The `libc` workspace
  dependency line added during drafting is now unused and was reverted in
  the final state check.

**Live proofs (production XDG layout):** (1) host SIGKILLed mid-run → child
died via SIGPIPE, record persisted, next spawn reaped the stale record;
(2) idle owned-shape orphan with a dead-host record → **reaped by the next
spawn**; (3) identical child with a **live** host record → **never
touched** (receipt ran to PASS beside it); (4) every scenario ended with
zero flm processes, zero registry entries, ports free.

## 5. Verification receipts (final build)

Candidate: `target/voice-candidates/agent-vesper-tui-verify-read-repair`
(SHA-256 `b289a6c1537ecaad…`, byte-identical to `target/release`; an earlier
in-session digest `b61c6233…` was superseded by the default-build cfg fix
below), features `voice-flm,voice-kokoro,voice-conversation`.

The final rebuild also repaired a default-build compile defect this unit's
receipt line introduced: `record_last_stt_route("CPU sidecar")` in
`voice.rs` was reachable without `voice-flm`, referencing a module compiled
only under `voice-conversation`; the line is now cfg-gated to the same
feature pair as the adapter branch above it (default build compiles clean
again — verified).

| Gate | Result |
|---|---|
| `voice_flm_route` (13 tests incl. the 2 new red-first) | ok ×4 parallel runs |
| TUI lib (`voice-flm,voice-kokoro,voice-conversation`) | 281 passed |
| All TUI integration test targets (12 suites) | all ok (6/10/6/10/12/3/13/5/11/2/7/2) |
| `flm_stt_receipt` (real installed model, clean device) | silence 0.204 s · speech 3.5–4.4 s · warm 2.7–3.3 s · PASS |
| **Native Verify + F9 PTY** (`flm_f9_loop_pty.py`) against the candidate | **PASS** — Settings save → Verify (Verified) → NPU select → Save → F9 → FLM adapter → one agent turn; `provider requests=2`; CPU recognizer untouched |
| `r3_loop_pty.py` (CPU F9 continuity) | PASS |
| `voice_pty.py` (venv python arg) | PASS |
| clippy `--all-targets` | 0 warnings |
| `cargo fmt` / default-build compile | clean |

**Verified Settings sequence for Alex** (the one to retry): Settings →
Voice → Accelerated recognition → **Verify** → expect *Verified: the
accelerated recognizer answered a local check…* → Speech recognition
compute → **NPU required** → Save → F9. Verification is per-process; a
restarted app re-runs Verify once.

### Honest invocation note

Two earlier harness failures were my own wrong invocations, not
regressions: `voice_pty.py` requires the **voice-venv python** as its
second argument (documented in `voice-latency-repair.md`; with the fixture
*.py* file passed instead, the recorder shebang is non-executable → the
correct `PermissionDenied` surfaced). `settings_pty.py` fails at its
`Swarm` submenu click **identically on every preserved candidate back to
Sep 21** (pre-existing environmental drift, root-menu cursor visible after
click; no submenu open). Both are recorded here rather than silently
dropped; neither is in this unit's scope, and the equivalent
Settings→Voice→save coverage passes in the FLM harness.

## 6. Residuals and open items

- **Live-user acceptance status after this unit:** recorded separately in
  [`voice-npu-user-acceptance.md`](voice-npu-user-acceptance.md) (2026-09-23):
  Alex reported the NPU-enabled conversation working with no apparent gaps and
  natural sound. That verdict does not change this unit's technical scope; the
  "Alex's live-microphone acceptance" line below was accurate at repair time
  and is superseded by that record only as a present status.

- **Pre-repair orphans cannot be reaped by the new code** (they predate the
  registry). All known ones were cleaned this session by exact PID with the
  evidence chain above; the machine ended clean.
- If Alex manually launches FLM servers outside Vesper, contention can still
  starve Verify — now reported as *"owned ASR process exited while
  answering"* rather than a false timeout, and recovery is one Verify retry
  after the foreign workload exits.
- PTY-harness hosts are SIGKILLed at test end; their temp-rooted registries
  die with the tempdir, so test-session children can still leak one at
  teardown (observed once; cleaned). Production hosts (persistent XDG) get
  the full registry/reaper protection. A PDEATHSIG-style kernel guarantee
  would close even this and remains available as a separately reviewed
  unsafe change if Alex wants it.
- Placement evidence stays process/device/model correlation (no per-request
  offload field exists); natural-speech accuracy and Alex's live-microphone
  acceptance remain open; the ~745 MB Llama residue and the vendor
  fallback-download question remain pending decisions.
- Unrun: full workspace suite, MSRV, CI, five-target matrix.
- R16 remains **not complete**; this unit closes the Verify reliability
  defect only. CPU STT, Kokoro TTS, the two-unit bank and all accepted
  continuity behavior are untouched and re-verified green.
