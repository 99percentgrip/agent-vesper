# VRO-17 PR-2 Execution — Synthesis-Only TTS, Hygiene/Gating, Bounded Storage

Work unit: engine-independent hygiene + sentence gating, a
synthesis-only local TTS adapter gated on existing policy, storage
budgets, and storage-safety proofs. **Stopped at PR-2.** No PR-3
orchestration, host integration, playback, capture, new STT, cloud
selection, model downloads, or release work. Nothing was cleaned or
deleted; `cargo clean` was not run.

## 1. Baseline

- Revision `94ed16de502e98110498010b399b24659b17a63f` on `main`, with
  all prior VRO-17 work uncommitted and preserved (PR-0/PR-1 crate +
  docs + guards + the existing-stack recon). The installed-binary/source
  string match from the recon is treated as **supporting evidence
  only**, not proof of exact build equivalence; running instances were
  not touched or rebuilt.
- Input-side baseline honored: F5/footer capture, VAD binding, editable
  composer insertion, no auto-submit — untouched (dictation suites
  re-run green: 8/8). No second capture owner, script, environment, or
  model cache was created; PR-1's `stt-sidecar` remains the single STT
  inference path. Host re-wiring and the shared-adapter decision stay
  with PR-4.

## 2. Storage receipt (measured; binary MiB/GiB)

Before implementation builds (measured `2026-09-18 ~22:0x`, read-only
`df`/`du`; `CARGO_TARGET_DIR` unset → workspace `target/`):

| Surface | Measured | Notes |
|---|---|---|
| `/home` (`/dev/nvme0n1p3`, btrfs) available | **277 068.9 MiB free** | same filesystem as `/` (two mounts, one device — counted once) |
| `/tmp` (tmpfs) available | 13 617.0 MiB | RAM-backed |
| `target/` (dev artifacts) | 1 205 998.1 MiB (of which `target/debug` 1 186 232.9) | pre-existing; PR-2 reuses it — no duplicate target dirs/worktrees |
| `~/.cargo/registry` | 1 099.7 MiB | shared; `~/.cargo/git` absent |
| `~/.rustup` | 2 844.0 MiB | toolchains |
| `voice-venv` (reusable STT env) | 391.2 MiB | **reused, not copied** |
| `~/.cache/huggingface/hub` (models) | 215.6 MiB | reused; **zero new downloads** |
| `/usr/share/espeak-ng-data` | 23.0 MiB | system engine data; **used in place, not copied** |
| Our prior `/tmp` residuals (PR-1 tests/probe) | 4.6 MiB | pre-existing owned artifacts; not deleted (no cleanup approval) |

After this unit (same tools): `/tmp` available **13 625.9 MiB** (probe
artifacts cleaned; +8.9 MiB net from new test fixture runs, cleaned
per-run); `/home` 276 997.5 MiB (−71.4 MiB ≈ incremental build growth
inside the existing `target/`; no new persistent voice files anywhere).
**Peak probe aggregate: 334 KiB** (three bounded WAVs, largest 137 KiB —
far under the 8 MiB/file and 32 MiB aggregate ceilings); **residual
after cleanup: 0 bytes** (verified: probe dir absent). No
voice-specific persistent diagnostic files were created (default off;
none needed — 0 MiB against the 4 MiB ceiling). Reserve check: ≥1 GiB
free after all planned writes — satisfied with five orders of magnitude
of headroom (276.9 GiB free). Excluded/unmeasured: unrelated home
contents (not scanned), symlink targets outside these roots, other
concurrent processes' writes (attribution of whole-filesystem deltas to
PR-2 is therefore not claimed). Logical sizes reported; btrfs
allocation differences (compressed/CoW) are why "available" is the
planning number here.

Storage-policy compliance for surfaces this unit controls:
production TTS audio files / persistent response-audio cache — **zero
files created, by construction and by test**; duplicate
models/envs/engine binaries — zero copies; new speech/model downloads —
none; probe files — 334 KiB peak, cleaned; persistent voice logs — none
created. Ordinary session-history retention is untouched and explicitly
**not** claimed as bounded by these voice limits.

## 3. Engine gate (§3 of the directive)

Assessed `espeak-ng` on this machine, specifically in **no-playback
mode** (`--stdout` captures the waveform; no audio device is opened —
verified by the probe's 0-speaker run):

- Installed: `espeak-ng-1.52.0-3.fc44.x86_64`; engine self-reports
  1.52.0, data at `/usr/share/espeak-ng-data` (23.0 MiB).
- Output contract verified empirically (probe receipts): WAV, PCM
  (audio format 1), **mono, 22050 Hz, 16-bit**, with RIFF/data size
  fields as **streaming placeholders** (`0x7FFFF...`) because stdout is
  not seekable — the adapter validates the container but ignores those
  sizes and bounds reads itself. Conversion 22050→16000 is implemented
  in-adapter (bounded linear resampler, tested); the engine's rate is
  never relabeled.
- License/deployment boundary (recorded separately from Cargo): the
  RPM declares `GPL-3.0-only AND GPL-3.0-or-later AND Apache-2.0 AND
  BSD-2-Clause AND Unicode-DFS-2016 AND CC-BY-SA-3.0`; upstream COPYING
  is GPL-3.0. **Policy determination:** Vesper invokes a user-installed
  binary at a process boundary via pipes and **distributes nothing** —
  no engine binary or voice data is bundled, copied into app data, or
  vendored (`deny.toml` untouched; zero new Cargo dependencies — the
  feature is `tts-subprocess = []`). Executing a system program is not
  distribution of that program; this matches the PRD §2.7
  option-(b) shape and required **no license exception**. Caveat
  recorded honestly: this is an integration-shape determination for
  *this* invocation pattern only, not approval to redistribute engine
  binaries/data or to link them; portability is per-machine (another
  user without the executable gets truthful `Unavailable` naming the
  install prerequisite — tested).
- **Gate verdict: PASS for an optional system-engine baseline.** Labeled
  exactly as the directive requires: not approval of final voice
  quality, not a neural model, not pure-Rust inference, not NPU.
  Verified on-device voices only (`-v` with the engine's own data; no
  alternative engines, no network routing).
- Markup: `-m` (SSML) is **never passed** — interpretation disabled.

## 4. Implemented

- `src/hygiene.rs` — the stateful, bounded hygiene+sentence gate
  (PRD §2.5 ordering frozen: hygiene on gated segments; protected spans
  tracked across chunk *and* sentence boundaries with progressive
  terminator matching; PEM tail-line discard with a newline/space
  boundary; skip markers carried to the next emitted unit so nothing is
  lost when a sentence vanishes; budget overflow is a loud
  `BudgetExceeded` that never flushes unvalidated fragments; finalize
  is exactly-once and never speaks an unterminated protected span;
  credential key/value + opaque-blob redaction with truthful markers;
  inline-code/link unwrapping; whitespace collapse; CJK `。` sentence
  ending). 16 unit tests, including the
  every-character-chunk-boundary equivalence proof and the
  instrumented-dispatch boundary proof.
- `src/tts_subprocess.rs` (feature `tts-subprocess`, default-off, zero
  deps) — **synthesis-only** adapter: fixed argv (`-v <voice> --stdin
  --stdout`), text on bounded stdin, EOF-signaling pipe close, own
  process group with `Drop` reaping exactly that child, per-request
  deadline → `Unavailable`, nonzero exit → `Inference`, non-WAV →
  `InvalidInput`, odd trailing byte → `Truncated`, over-bound output →
  `ResourceExhausted`, WAV container validation (placeholder size
  fields deliberately ignored), 22050→16000 conversion, **no audio
  files anywhere** (stdout→memory only), pre-start cancellation never
  spawns, mid-stream cancellation suppresses remaining audio
  (`FrameStream` + `TtsMidStreamError::Cancelled`), child
  serialization via a guard inside the blocking closure (future stays
  `Send`), `streaming: false` advertised truthfully (whole-request
  buffering before frames yield; reading stdout ≠ low latency). Process
  exit is never treated as playback evidence; no playback ack exists.
  Missing engine → `Unavailable` naming the install prerequisite.
- `tests/pr2_tts.rs` — 19 tests over the real adapter code with
  deterministic subprocess fixtures (ok/nonzero/malformed/truncated/
  oversized/slow) plus the storage-safety suite.
- PRD amendments: D23 (synthesis/playback separation contract),
  R17–R19 storage requirements (§PRD updates below), PR-2 section
  status; historical decisions untouched, no renumbering.

## 5. Real-engine no-speaker probe (receipts on disk)

`docs/foundation/voice-oracle-pr2-probe.py` → `-probe-results.json`.
Real `espeak-ng` 1.52.0, fixed argv identical to the adapter's,
nonsensitive text, speaker never activated:

| Text | First output | Total gen | Format | Duration |
|---|---|---|---|---|
| "Testing synthesis." | 7.6 ms | 8.8 ms | WAV/PCM mono 22050 Hz s16 | 1.392 s |
| sentence pair | 5.4 ms | 8.2 ms | same | 3.181 s |
| code-skipped (hygiene-shaped) | 5.4 ms | 7.9 ms | same | 3.162 s |

Valid nonempty audio proves **synthesis output** — not naturalness,
intelligibility, or playback; no listening check was performed
(unperformed, marked as such in the receipts). First-output ≈ total
time confirms buffered generation (matches `streaming: false`).

## 6. Storage-safety tests (executed; no real disk filling)

- `synthesis_creates_no_files_anywhere`: before/after snapshot of the
  temp root around a full successful synthesis — identical.
- `repeated_synthesis_does_not_accumulate_children_or_files`: 10
  sequential runs — identical snapshots, bounded time.
- `bounded_output_under_stalled_consumer`: stream dropped without
  draining; cleanup bounded; a follow-up synthesis succeeds (no leaked
  lock/child).
- `cleanup_after_failure_paths_leaves_no_owned_files`: all four failure
  fixtures — identical snapshots after each.
- `optional_write_refusal_is_represented`: the low-space/unknown-space
  refusal contract (future optional writes refuse rather than switch
  destinations) pinned as an executable contract.
- `no_isolation_violations_no_symlink_escape`: structural source
  assertion — the adapter contains no `remove_file`/`remove_dir`/
  `read_dir`/`canonicalize`/`walkdir` at all; it never manages files.
- Fixed repetition count: 10 iterations; max temporary usage 0 bytes
  beyond fixtures (in-memory PCM); residual owned files 0; retained
  log size 0. Failure injection is fixture-based (fixture engines
  emit oversized/truncated/malformed output); **Alex's SSD was never
  filled to test exhaustion.**
- No file-spooling subsystem was built to test one: the adapter's
  zero-file behavior is the proof.

## 7. Verification (final state, storage-aware policy)

| Gate | Command | Result |
|---|---|---|
| Hygiene unit tests | `cargo test -p vesper-voice --lib hygiene` | 16/16 |
| PR-2 adapter suite | `cargo test -p vesper-voice --features tts-subprocess --test pr2_tts` | 19/19 (2.0 s) |
| Full voice crate | `cargo test -p vesper-voice --features stt-sidecar,stt-http,tts-subprocess` | 59+13+30+19 = 121/0 |
| Workspace all-features | `cargo test --workspace --all-features` | **171 targets, 2604/0** |
| Workspace default | `cargo test --workspace` | 168 targets, 2449/0 |
| Doc tests | `cargo test --workspace --doc` | 28 targets, 0 failed |
| Dictation regression | `cargo test -p agent-vesper-tui voice` | 8/8 |
| Strict Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| fmt | `cargo fmt --all --check` | clean |
| Architecture | `cargo xtask architecture` | 29 packages (no dep changes) |
| Naming guard | `cargo xtask naming-guard` | clean, 36 frozen |
| Acceptance | `cargo xtask acceptance` | 23/23 (6.2 s) |
| Pure-core bare build | `cargo build -p vesper-voice --no-default-features` | clean |

Build/storage policy: reused the existing `target/` (no duplicate
dirs/worktrees/release builds/matrices); growth ≈71 MiB incremental on
a 277 GiB-free filesystem with the ≥1 GiB reserve trivially intact;
space was checked before and between batches via `df`. No caches were
cleared and no artifacts removed to shrink receipts. Cargo's own
dependency retrieval during builds is normal approved behavior and is
included in the above accounting (no speech-model downloads occurred).

## 8. Phase boundary: capture storage (inspected, not altered)

Shipped dictation's capture retention was inspected read-only to
identify its owner: `apps/agent-vesper-tui/src/voice.rs` `Audio`
(tempdir + WAV, deleted on success/discard, retained on failure for
Retry/Discard) — owner is the TUI worker. **Gap recorded as
unverified/outstanding**: no explicit capture-duration/file-size cap or
aggregate-across-instances bound exists in the shipped path; PR-2 tests
cannot certify it. Binding PR-4 acceptance items have been added to the
PRD (below). Nothing in shipped dictation was modified. Future model
setup remains responsible for declared download sizes, verified asset
reuse, destination-space checks, and explicit download approval.

## 9. Unmet gates (unchanged or newly visible; none waived)

- Positive-STT-recognition receipt (speech fixture) — open since PR-1;
  this synthesis probe does **not** satisfy it.
- Cloud TTS and R3's local+cloud completion gate — untouched; no
  cloud-only fallback introduced; **R3 remains unmet**.
- Cross-target/MSRV/CI — pending CI (green local Linux run certifies
  neither other targets nor release readiness).
- Neural-quality local voice and final product voice selection —
  Alex's; the system-engine route is explicitly a baseline only.
- PR-4 capture/host acceptance items (new, below) — future binding.

## 10. PRD updates (traceable, no renumbering of R1–R12)

- **D23 (PR-2)**: synthesis and playback are separate concerns; a
  synthesis adapter returns validated PCM and never speaks, plays, or
  delegates untracked playback; process exit is never playback
  evidence; no fabricated playback acks. Evidence: adapter code +
  `mid_stream_cancellation` + no-file proofs.
- **R17**: production TTS creates zero audio files/persistent caches
  (proved by test; receipts §2/§6).
- **R18**: probe artifacts ≤8 MiB/file, ≤32 MiB aggregate, cleaned
  after every outcome; residuals 0 (proved: 334 KiB peak, cleaned).
- **R19**: future optional voice disk writes refuse when free space is
  unknown or below the ≥1 GiB reserve (contract test pinned).
- **R20** *(binding PR-4, from §8)*: dictation capture must gain
  explicit duration/file-size caps, aggregate temp accounting across
  dictation/conversation instances, and cleanup-after-every-outcome +
  crash-recovery acceptance — recorded as **outstanding**, not
  satisfied by PR-2.
- Status line + PR-2 section updated to COMPLETE (implementation);
  acceptance for PR-2's own scope is complete, while R3/R20 and the
  PR-1 speech receipt remain explicitly open.

## 11. Readiness

PR-3 (voice-session orchestration: sentence-unit coordination with
agent turns, barge-in wiring) can start — its inputs (hygiene gate,
`SpeakUnit`, TTS port with mid-stream semantics, cancellation
hierarchy) are all landed and gated. Reused: the entire input-side
stack, PR-0/PR-1 seams, and the system engine in place. Added:
hygiene/gating + the synthesis-only adapter + storage proofs.

**Stop at PR-2.** The success claim is exactly: a verified
text-to-PCM component and hygiene pipeline with measured, bounded
voice-owned storage — not a speaking TUI, not completed bidirectional
voice, and no guarantee that the SSD cannot fill through other means.

## Verification

- §7 gates run on the final source state after all fixes.
- No microphone, speaker, or remote service was used; no models
  downloaded; no user data deleted; running instances untouched.
- Historical receipts preserved; this report's storage numbers are
  measurements with scope and time, not estimates presented as data.
