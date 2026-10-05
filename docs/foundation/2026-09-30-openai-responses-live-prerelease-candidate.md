# OpenAI Responses live prerelease candidate — 2026-09-30

## Status

**NON-PUBLISHING LINUX DEBUG CANDIDATE BUILT FROM A CLEAN ISOLATED REPAIR COMMIT. ALL 60 OPENAI ADAPTER TESTS AND BOTH FOCUSED TUI FAILURE-SURFACE TESTS PASSED. THE CANDIDATE COMPLETED ONE REAL OPENAI TURN AND RENDERED A SUCCESSFUL LAST RUN. NO RELEASE, PUBLICATION, TAG, PUSH, INSTALLATION, RELEASE-PROFILE BUILD, RRC ACTION OR VRO-19 WORK OCCURRED.**

## Objective

Build a fresh prerelease `agent-vesper-tui` executable from the exact OpenAI Responses streaming-boundary and error-surface repair, outside the repository/worktree, then run that exact artifact directly against the configured local OpenAI runtime. Preserve:

- `wire::MAX_EVENT == 1_048_576`;
- per-line and per-event SSE bounds without cross-event accumulation;
- fail-closed unknown event handling;
- partial-output retention, ambiguous-tool interruption and no replay;
- concise bounded safe text in chat and Last Run;
- bounded, secret-scrubbed structured diagnostics only in activity/debug.

The operation was explicitly limited to a debug prerelease candidate and live validation. It did not authorize a release, installer, publication, tag, push, candidate release matrix, RRC workflow or VRO-19 work.

## Source and artifact identity

Requested worktree baseline:

```text
worktree=/home/Alex/Projects/agent-vesper/.worktrees/rrc-resource-governor
base_commit=01d2df045a7c30253bea8e613e9840b9f62d053b
workspace_version=0.24.4
```

The inherited worktree also contained unrelated RRC/VRO edits. To prevent those edits from entering the executable, the candidate source was reconstructed outside the repository from the base commit plus only these repaired production/test files:

```text
apps/agent-vesper-tui/src/main.rs
crates/vesper-agent/src/agent_loop.rs
crates/vesper-provider-openai/src/tests.rs
crates/vesper-provider-openai/src/wire.rs
```

The isolated tree was committed locally only:

```text
candidate_commit=49cf424fea4593bb63acf9ea03eb063ffd04de14
parent_commit=01d2df045a7c30253bea8e613e9840b9f62d053b
subject=fix(openai): isolate responses diagnostics
4 files changed, 549 insertions(+), 18 deletions(-)
```

External roots:

```text
source=/home/Alex/Projects/agent-vesper-openai-live-source-20260930T172641Z
build=/home/Alex/Projects/agent-vesper-prerelease-builds/openai-responses-live-20260930T172641Z
candidate=/home/Alex/Projects/agent-vesper-prerelease-candidates/openai-responses-live-20260930T172641Z
```

Candidate executable:

```text
path=/home/Alex/Projects/agent-vesper-prerelease-candidates/openai-responses-live-20260930T172641Z/bin/agent-vesper-tui
version=agent-vesper-tui 0.24.4
profile=dev/debug
sha256=da11e8f8bd0c9ce0efb81aad545a79f444250b71601895fde9324729697096bb
format=ELF 64-bit LSB pie executable, x86-64, dynamically linked, with debug_info, not stripped
```

The staged executable and the executable in the isolated Cargo target were byte-identical:

```text
da11e8f8bd0c9ce0efb81aad545a79f444250b71601895fde9324729697096bb  .../target/debug/agent-vesper-tui
da11e8f8bd0c9ce0efb81aad545a79f444250b71601895fde9324729697096bb  .../candidate/bin/agent-vesper-tui
cmp_exit_status=0
```

The exact repair patch is retained at `source-repair.diff` in the candidate directory:

```text
sha256=51db060f72d40e45ab5f16c5d547bed69051f259c1daf84f1d44c31fd0cea302
```

## Methods and commands

1. Re-read the root, app, TUI, crate, OpenAI-adapter, documentation and foundation DOX chain.
2. Recorded the requested worktree identity and preserved its unrelated dirty changes.
3. Created a clean external source tree from commit `01d2df045a7c30253bea8e613e9840b9f62d053b`.
4. Applied only the four-file OpenAI repair to that source tree.
5. Added defense-in-depth activity scrubbing with `vesper_agent::vro::SecretScrubber` and a deterministic bearer-token canary assertion. The canary must be absent from transcript, Last Run and activity raw text; activity must contain `[REDACTED:BEARER_TOKEN]`.
6. Used a dedicated external Cargo target and the shared Cargo cache:

```text
CARGO_HOME=/home/Alex/.cargo
CARGO_TARGET_DIR=/home/Alex/Projects/agent-vesper-prerelease-builds/openai-responses-live-20260930T172641Z/target
```

7. Ran the focused and complete affected tests.
8. Built `cargo build -p agent-vesper-tui --bin agent-vesper-tui` in the debug/dev profile.
9. Copied that exact executable to the external candidate directory and verified it byte-for-byte against the build output.
10. Launched the staged candidate by its absolute path in a PTY. The successful run set `AGENT_VESPER_HOME=$HOME/.agent-vesper` to reuse the already-selected OpenAI provider and its existing native credential storage; no credential was printed or copied.
11. Captured the raw PTY stream, produced an ANSI-stripped transcript and validated the rendered provider/model, assistant response, Last Run result, provider-turn count and clean process exit.

## Verification receipts

### Complete OpenAI adapter suite

```text
$ cargo test -p vesper-provider-openai --features integration-test-harness --lib

running 60 tests
...
test tests::http::sse_aggregate_over_one_mib_across_events_does_not_accumulate ... ok
test tests::http::sse_semantic_rejection_can_report_exact_bound_after_bounded_wire_input ... ok
test tests::http::malformed_after_visible_text_and_tool_start_emits_one_terminal_without_replay ... ok
test tests::malformed_event_diagnostics_are_bounded_and_secret_safe ... ok
...
test result: ok. 60 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
openai_exit_status=0
```

This run includes the direct loopback matrix for one-byte/coalesced transport chunks, LF/CRLF boundaries, multiline data, SSE control fields, incomplete EOF, aggregate traffic above 1 MiB, exact/oversized events, exact-bound canonical JSON diagnostics and interrupted no-replay settlement.

### TUI safe-surface and secret-canary suite

```text
$ cargo test -p agent-vesper-tui --bin agent-vesper-tui provider_failure -- --nocapture

running 2 tests
test tests::provider_failure_keeps_safe_text_separate_from_structured_diagnostics ... ok
test tests::provider_failure_and_timeout_never_become_user_cancellation ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 165 filtered out
tui_exit_status=0
combined_exit_status=0
```

The first test supplies structured protocol fields plus this secret-shaped fixture:

```text
Bearer sk-live-test-SecretCanary0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ
```

Assertions prove:

- chat/transcript contains only `OpenAI returned malformed or oversized Responses data`;
- chat/transcript does not contain `ProviderError`, `responses-event`, `<unrecognized>` or the canary;
- Last Run does not contain debug formatting, structured diagnostics or the canary;
- activity does not contain the raw canary;
- activity contains `[REDACTED:BEARER_TOKEN]` and retains the bounded structured `responses-event` diagnostic.

### Build

```text
$ cargo build -p agent-vesper-tui --bin agent-vesper-tui
Compiling agent-vesper-tui v0.24.4 (.../apps/agent-vesper-tui)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.85s
exit_status=0
```

### Direct candidate OpenAI runtime

The candidate was launched directly from:

```text
/home/Alex/Projects/agent-vesper-prerelease-candidates/openai-responses-live-20260930T172641Z/bin/agent-vesper-tui
```

The prompt was:

```text
Reply with exactly OPENAI_LIVE_CANDIDATE_OK and nothing else.
```

The ANSI-stripped terminal capture contains these verbatim receipts:

```text
openai·gpt-5.6-terra·v0.24.4
• OPENAI_LIVE_CANDIDATE_OK
✓ Turn complete
Provider turns  1
Tool results    0
Elapsed         2.6s
```

The process then accepted `/quit`, saved the session and exited with status 0. The post-capture validator reported:

```text
candidate_launched_by_absolute_path=true
openai_provider_and_model_visible=true
assistant_response_visible=true
last_run_turn_complete=true
one_provider_turn=true
clean_process_exit=true
validation_exit_status=0
```

This is a successful normal live OpenAI turn. It is not a replay of the unavailable historical unknown event and does not claim that the provider emitted that event again.

## Candidate contents

- `README.md` — direct-run instructions and non-installing boundary.
- `manifest.json` — base/candidate commits, source hashes, toolchain, host, binary hash and operation boundaries.
- `SHA256SUMS` — hashes for all 47 retained candidate/forensic files; final verification returned 0.
- `evidence/` — a self-contained copy of this report plus its owning workspace path.
- `source-repair.diff` and `source-repair.diff.sha256` — exact isolated repair.
- `bin/agent-vesper-tui` and its SHA-256 file.
- `logs/` — test, build, identity, diff and binary-comparison receipts.
- `runtime/live-openai-tui-2.raw` — raw successful OpenAI PTY capture.
- `runtime/live-openai-tui-2.stripped.txt` — ANSI-stripped successful capture.
- `runtime/live-openai-validation.txt` — successful post-capture validator receipt.
- Earlier failed-attempt captures retained under `runtime/` rather than deleted.

## Files changed in the worktree

### Product/test repair

- `apps/agent-vesper-tui/src/main.rs` — provider-safe chat/Last Run text, separate bounded activity diagnostic, defense-in-depth secret scrubbing and secret-canary regression.
- `crates/vesper-agent/src/agent_loop.rs` — safe provider error display.
- `crates/vesper-provider-openai/src/tests.rs` — SSE framing, exact-bound and no-replay matrix.
- `crates/vesper-provider-openai/src/wire.rs` — diagnostic byte-basis/expected-shape clarification while preserving the 1 MiB bound.

### Completion evidence

- this report;
- `docs/foundation/evidence-index.md`;
- `docs/openai-provider-prd.md`;
- `docs/AGENTS.md`;
- `docs/foundation/AGENTS.md`.

## Deviations

- The first candidate PTY attempt set a candidate-local checkpoint root. In this revision that also prevented loading the user-wide provider preference, so the TUI defaulted to Z.ai and ended with the safe message `GLM request failed`. The first driver also matched the expected response token in its own typed prompt before the provider settled. That attempt is not counted as OpenAI evidence.
- A second attempt without `AGENT_VESPER_HOME` again defaulted to Z.ai because provider selection in this source revision is rooted at `.agent-vesper` unless the home is explicit. It also ended with `GLM request failed` and is not OpenAI evidence.
- The successful OpenAI run's PTY driver watched for the obsolete phrase `Agent turn completed`. The current UI rendered `✓ Turn complete`, so the driver itself timed out and returned 2 after the candidate had completed and later exited cleanly. No additional paid call was made merely to correct the watcher. A post-capture validator checked the actual rendered receipt and returned 0. Both the driver mismatch and the successful capture are retained.
- An early `cargo test -p vesper-provider-openai --lib sse_` filter selected zero tests because the framing tests are feature-gated under `tests::http`. It was replaced by the correct `--features integration-test-harness` run, first for all 18 HTTP tests and finally for all 60 adapter tests.
- A proposed agent-loop test-name filter also selected zero tests because no test has that name. It is not cited as evidence. No source change was made to manufacture a matching test.
- The candidate is debug/dev, as requested. No release-profile executable was built.

## Unresolved items

- The original provider event bytes and exact unknown discriminant were not retained. This candidate cannot replay the historical failure exactly.
- The live run proves that the exact candidate can start with the configured OpenAI provider, complete one normal Responses turn and render truthful Last Run status. It does not force a current unknown event or independently prove every failure surface with live provider bytes; those surfaces are covered by deterministic loopback and secret-canary tests.
- Validation is Linux x86_64 local evidence only. No MSRV, five-target, canonical foundation, contained web-driver, release or publication workflow ran.
- The isolated candidate commit is local and detached. It was not pushed or tagged.
- The user's installed `agent-vesper-tui` was not replaced. Update/install acceptance remains untouched.

## Readiness effect

A runnable, hash-addressed debug candidate now exists outside the repository and includes only the requested OpenAI repair over base commit `01d2df045a7c30253bea8e613e9840b9f62d053b`. The deterministic suite protects the one-event-at-a-time SSE lifecycle, exact 1 MiB distinction, fail-closed semantic rejection, no replay and safe-surface separation. The added defense-in-depth scrubber proves a secret-shaped canary remains absent from chat, Last Run and raw activity while a redacted placeholder remains in bounded activity diagnostics. The staged binary completed one real OpenAI turn and exited cleanly.

This is sufficient for the requested local debug prerelease/runtime validation. It does **not** authorize or establish release readiness, cross-platform readiness, publication, installation, exact reproduction of the unavailable historical event, RRC progression or VRO-19 progression.
