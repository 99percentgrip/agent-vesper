# VRO-17 release-readiness audit — 2026-09-24

## Objective

Perform the requested end-to-end acceptance audit of
[`docs/voice-oracle-extraction-prd.md`](../voice-oracle-extraction-prd.md),
repair concrete blockers found in the acceptance machinery, verify one immutable
combined-feature Linux release candidate, and state separately:

1. whether the amended R1–R20 product requirements are implemented at their
   documented scope; and
2. whether the repository is ready for a public release.

This unit did **not** use a real microphone, speaker, public provider, installed
application replacement, release/tag/push path, or Settings/model mutation in
Alex's normal user state. Existing live user sessions were not interrupted.

## Baseline and candidate identity

- Source baseline: `main` at `8f258ba28f4e` plus the existing dirty VRO-17
  working tree (`182` changed/untracked paths at closeout; `133` matched the
  voice/Kokoro/FLM path filter). This is not an exact commit and is not covered
  by prior release CI.
- Candidate path:
  `target/acceptance/agent-vesper-tui-vro17`
- Build command:
  `cargo build --release -p agent-vesper-tui --features voice-flm,voice-kokoro`
- Candidate SHA-256:
  `8895a711b4cb19c4f37eebc7b4d57d02e12f334e796f5f91d577c91d0d7e6398`
- Candidate size: `22,685,640` bytes.
- Candidate format: stripped x86-64 Linux PIE ELF.
- `target/release/agent-vesper-tui` and the copied acceptance candidate were
  byte-identical at build time.
- This exact identity applies to the no-device acceptance runs below. It does
  **not** retroactively identify the executable used for Alex's 2026-09-23
  listening statement; that historical artifact association remains unknown.

## Methods and commands

### Contract and source audit

The PRD was re-read line by line against:

- the production voice composition and key handlers in
  `apps/agent-vesper-tui/src/`;
- the provider-neutral core and Kokoro adapter;
- every current voice integration suite under
  `apps/agent-vesper-tui/tests/`;
- the R20, R6, continuity, NPU, user-listening, and previous final-audit
  records;
- the current migration status, evidence index, and applicable DOX chain.

An independent delegated review was attempted. It did not run because the
configured reviewer provider rejected the selected OpenAI model as unavailable:
`Selected OpenAI model is not in the current account model list`. No independent
approval is fabricated; this limitation remains explicit.

### Automated regression gates

Final post-repair commands:

```text
cargo test -p agent-vesper-tui --features voice-conversation --test voice_r20_default_capture
cargo test -p vesper-voice-kokoro --features ort --lib --tests
cargo test -p agent-vesper-tui --features voice-flm,voice-kokoro --lib --bin agent-vesper-tui --tests
cargo test -p agent-vesper-tui --no-default-features --lib --bin agent-vesper-tui
cargo clippy -p agent-vesper-tui --features voice-flm,voice-kokoro --lib --bin agent-vesper-tui --tests -- -D warnings
cargo fmt --check
cargo xtask architecture
cargo xtask naming-guard
cargo xtask acceptance
sh scripts/test_install_upgrade.sh
```

### Production-path, no-device harnesses

All harnesses used the exact candidate above, isolated HOME/config/data roots,
loopback provider fixtures, controlled recorder/player sinks, and the existing
verified local voice assets. They did not open a real microphone or speaker.

```text
python3 apps/agent-vesper-tui/tests/voice_pty.py \
  target/acceptance/agent-vesper-tui-vro17 \
  "$HOME/.local/share/agent-vesper/voice-venv/bin/python"

python3 apps/agent-vesper-tui/tests/r3_loop_pty.py \
  target/acceptance/agent-vesper-tui-vro17 \
  "$HOME/.local/share/agent-vesper/voice-venv/bin/python" \
  am_michael <off|balanced|list|continuity|preview>

python3 apps/agent-vesper-tui/tests/flm_f9_loop_pty.py \
  target/acceptance/agent-vesper-tui-vro17 \
  "$HOME/.local/share/agent-vesper/voice-venv/bin/python" am_michael

FLM_MODEL_PATH="$HOME/.config/flm" \
  target/release/examples/flm_stt_receipt --i-have-the-installed-model
```

## Exact verification receipts

### Rust and repository gates

- R20 default-build suite: `11 passed; 0 failed`.
- Kokoro `ort` library/tests: `42 passed; 0 failed`.
- Combined-feature TUI command executed all registered lib/bin/integration
  targets. Per-target receipts were:
  `281, 158, 6, 10, 6, 10, 12, 3, 13, 5, 11, 2, 7, 8, 11, 4, 2`
  passed, each with `0 failed`.
- The four-test R6 production-host suite included:
  `speaking_f9_is_one_gesture_genuine_barge_in`,
  `explicit_stop_stops_everything_and_opens_no_capture`, and
  `repeated_barge_in_cycles_recover`.
- Default/no-feature TUI: `251 passed; 0 failed` (library) and
  `156 passed; 0 failed` (binary).
- Strict combined-feature Clippy: clean with `-D warnings`.
- `cargo fmt --check`: clean.
- Architecture: `architecture boundaries validated for 30 packages`.
- Naming guard: `clean (36 hits, all frozen in baseline)`.
- Completion assurance: `23 exact cases passed`; live-model effectiveness was
  not measured.
- Installer upgrade fixture:
  `Installer upgrade preserves user state and database inode: PASS`.
- `shellcheck` was unavailable locally and is recorded as not run.

### F5 dictation

Verbatim final harness receipt:

```text
PASS: mouse/F5, managed 120-second/4-MiB capture cap, >9-second progressing multi-chunk transcription, editable composer, retry/discard, early exit, disk failure and shutdown cleanup
```

The success text was corrected during this audit because it still claimed the
superseded ten-minute/>90-second contract after R20 changed shipped capture to a
120-second/4-MiB bound. The exercised assertions already used the current bound;
the stale banner was evidence drift.

### CPU F9 conversation and Kokoro

Verbatim receipts from the exact candidate:

```text
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (off) -> fixture player; PCM bytes=[52800, 124800]; peaks=[20247, 16064]; provider requests=1
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (balanced) -> fixture player; PCM bytes=[52800, 124800]; peaks=[20247, 16064]; provider requests=1
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (list) -> fixture player; PCM bytes=[62400, 128000]; peaks=[18218, 20239]; provider requests=1
PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro am_michael (continuity) -> fixture player; PCM bytes=[353600, 278400]; peaks=[25064, 25919]; provider requests=1
PASS: actual Settings Preview -> real Kokoro -> fake player; click-to-first-PCM=2374.3 ms; peak=18625; no recorder/provider
```

Observed synthetic-loop timings, disclosed rather than promoted to device
latency: recorder onset about `250.5–251.0 ms`; stop-to-first-fixture-PCM
`4.44–5.65 s`; the loop deliberately includes PTY polling, synthetic STT,
a one-second provider hold, and real Kokoro inference. These are not acoustic or
public-provider end-to-end measurements.

### FLM/NPU F9 route

After the harness repair described below, two consecutive independent exact-
candidate runs produced:

```text
PASS: Settings save -> Verify -> F9 -> FLM NPU adapter -> one agent turn; provider requests=1; player PCM bytes=[92000]; CPU recognizer untouched
post-attempt-1: no accel0 holder
PASS: Settings save -> Verify -> F9 -> FLM NPU adapter -> one agent turn; provider requests=1; player PCM bytes=[92000]; CPU recognizer untouched
post-attempt-2: no accel0 holder
```

A standalone production-composition receipt after cleanup produced:

```text
silence: Ok(("", "VadConfirmedSilence")) in 0.215s
speech: Ok(("Ooooooooh. Ooooooooh. Ooooooooh. Ooooooooh. Ooooooooh. Ooooooooh. Ooooo", "InferredText")) in 3.708s
warm repeat: ok=true in 2.702s
PASS: composed FLM NPU recognition receipt (synthetic fixtures, no device)
```

At final observation, `/dev/accel/accel0` had no FLM holder and there was no
candidate or owned FLM process left by this audit.

## Findings and bounded repairs

### 1. Test-only Clippy blocker

Strict Clippy found `new_without_default` on the R6 `DoubleStt` test double.
A minimal `Default` implementation delegating to `new()` was added. The focused
R6 suite and strict combined-feature Clippy then passed.

### 2. FLM PTY chat-request accounting defect

The FLM loopback fixture counted every POST, allowing an optional embedding
request to race in after the one-chat-turn assertion and produce the internally
contradictory receipt `one agent turn; provider requests=2`. It now rejects
non-chat POSTs and counts only requests containing `messages`, matching the CPU
harness. The final receipt is `provider requests=1`.

### 3. FLM PTY teardown leaked this audit's owned helpers

The generic PTY close path terminates the TUI process group, while the production
FLM child intentionally owns a separate process group. Each test also used a new
isolated data root, so a later test could not read the preceding test's child
registry. Three exact helpers created by this audit remained on ports
`18131`, `18132`, and `18130`, held `/dev/accel/accel0`, and caused later Verify
attempts to fail truthfully as `owned ASR process exited while answering` or
`flm exited during startup`.

The three PIDs were identity-checked against the exact owned launch shape before
TERM/KILL cleanup. No user TUI or unrelated process was touched. A speculative
production retry was drafted during diagnosis, then fully removed once the
harness leak was proven; the final production binary returned to the original
hash. The harness now sends the application's normal Ctrl+X exit first, waits
for adapter Drop to reap the exact FLM child, then uses the generic close fallback
and deletes the isolated root. Two consecutive post-repair runs left no
accelerator holder and no new `.flm-loop-*` root.

Historical `.flm-loop-*` roots and three old recorder fixtures not created by
this audit were deliberately left untouched.

### 4. Stale acceptance documentation

The previous final audit and migration row still described R6 as implementation-
open even though the current tree contains the binding repair and production-
host regression. The current records are amended to distinguish:

- implementation/automated acceptance: PASS;
- new real-device interruption matrix: not rerun in this unit and not claimed;
- public release readiness: still separately open.

The previous audit also repeated obsolete F5 ten-minute language and a closed
R20 gap; those current-status claims are corrected while historical records stay
preserved as history.

## R1–R20 audit decision

The amended requirement ledger contains 22 rows because R16 has three separately
gated stages.

- **PASS:** R1, R2, R3, R4, R6, R8, R9, R10, R11, R12, R13, R15, R16c,
  R17, R18, R19, R20 — 17 rows.
- **PASS with documented scope limitation:** R5, R7, R14, R16a — 4 rows.
- **OPTIONAL / capability-gated and conformant on CPU:** R16b — 1 row.
- **OPEN implementation requirements:** none under the amended R1–R20 contract.

This is an implementation and controlled-acceptance decision, not a claim that
all device/release evidence exists. In particular:

- Alex's positive 2026-09-23 listening verdict remains valid at its recorded
  scope, but its exact executable remains unidentified.
- The exact candidate in this report is the future **automated regression
  baseline** for Linux combined-feature voice behavior. It is not relabeled as
  the historical user-tested artifact.
- No new microphone/speaker acceptance, quantitative natural-speech accuracy,
  acoustic gap measurement, repeated-interruption device matrix, or per-request
  NPU offload proof was performed.
- Kokoro synthesis remains CPU-backed; no NPU TTS route is claimed.

## Packaging and native-dependency review

- Candidate Cargo version: `0.23.3`.
- Feature closure from Cargo metadata:
  - `voice-conversation -> dep:vesper-voice`
  - `voice-flm -> voice-conversation`
  - `voice-kokoro -> voice-conversation, dep:vesper-voice-kokoro`
- ELF dynamic dependencies were limited to the expected system runtime set:
  `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`, and `ld-linux-x86-64.so.2`.
  ONNX Runtime and voice model/runtime assets remain optional per-user pack
  contents rather than new unresolved ELF dependencies.
- Installer upgrade-state preservation passed in its temporary fixture.
- No Linux desktop file, icon, or MIME registration artifact exists in the
  current package contract, so there was no applicable integration artifact to
  validate. This is recorded as not applicable, not silently counted as a pass.
- Release archive bundle-content workflows, MSRV, five-target builds, supply-
  chain gates, and exact-commit CI were not run in this unit.

## Deviations and unresolved items

1. Independent reviewer execution was unavailable because the selected reviewer
   model was not in the configured account catalog.
2. `shellcheck` was unavailable locally.
3. The final all-in-one visible test command was terminated by the execution
   environment after producing many green suites; every required gate was then
   rerun individually with bounded log output and passed. The interrupted run is
   not counted as evidence.
4. No exact commit contains the candidate source. Therefore canonical CI, MSRV,
   cross-target, package, installer, and release workflow evidence cannot yet be
   attached to these bytes.
5. PR-5 remains open: document the ACP interactive-terminal exclusion (or build
   a real ACP voice surface), add the cross-host exclusion/parity assertion,
   update user release documentation, commit the complete tree, and run the
   exact-commit release gates before tagging.
6. Device-level interruption/recovery acceptance and quantitative latency/
   accuracy remain unexecuted by explicit task constraint. They are not silently
   converted into automated passes.

## Readiness effect

- **Amended R1–R20 implementation:** accepted at the documented automated and
  previously recorded user-evidence scope.
- **Exact Linux candidate for controlled regression:** accepted; immutable path
  and digest recorded above.
- **Public release readiness:** **NO**. PR-5, independent review, exact-commit CI,
  MSRV/cross-target/package gates, and prohibited device evidence remain open.
- **Installation/release action:** none performed. Alex's installed application,
  settings, providers, models, and live sessions were not replaced or changed.
