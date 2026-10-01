# Post-release quality-check investigation

**Date:** 2026-09-29

**Status:** INITIAL REPAIR VERIFIED; LATER CI RACES OWNED BY FOLLOW-UP REPORT

**Affected run:** `pull-request-validation` run `36516876750` on commit `7b04588216a28cf7b62b379b157640bc6b0872d7`

**Repair commits:** `ef842957ce5c77db4a6c8ee49572ab958ba4f275`, `96cdb48a00c5fb492cb080e4646c973972a0491e`

## Objective

Investigate the single failed GitHub check shown after the v0.24.4 release closeout, identify the exact failing assertion and determine whether current evidence requires a source correction. Preserve the already released tag and artifacts, unrelated workspace changes, credentials, and Alex's local installation.

## Methods and commands

The investigation was read-only except for this evidence record and its documentation links.

```text
git status --short --branch
git remote -v
git log -5 --oneline --decorate
df -h / .
du -sh target
gh run list --limit 30 --json databaseId,name,workflowName,displayTitle,event,status,conclusion,headSha,createdAt,updatedAt,url
gh run view 36516876750 --json jobs,conclusion,headSha,url
gh run view 36516876750 --log-failed
gh run view 36516876750 --log-failed | tail -n 220
git show --stat --oneline 7b04588
git show --stat --oneline d221135
git log --oneline --all -- apps/agent-vesper-acp/tests/xai_native.rs
git blame -L 240,285 apps/agent-vesper-acp/tests/xai_native.rs
cargo test -p agent-vesper-acp --test xai_native --all-features grok_session_run_command_executes_once -- --exact  # repeated 20 times
cargo test -q -p agent-vesper-acp --test xai_native --all-features                          # repeated 50 times
```

Relevant source inspected:

- `.github/workflows/ci.yml`
- `apps/agent-vesper-acp/tests/xai_native.rs`
- `apps/agent-vesper-acp/tests/support/mod.rs`
- `apps/agent-vesper-acp/src/lib.rs`
- `crates/vesper-agent/src/tools.rs`
- `crates/vesper-agent/src/confinement.rs`

## Exact evidence

### Run and job result

GitHub identified one failed workflow for the post-release documentation commit:

```text
run: 36516876750
workflow: pull-request-validation
event: push
head: 7b04588216a28cf7b62b379b157640bc6b0872d7
conclusion: failure
quality job: 109241036120 — failure
supply-chain job: 109241035941 — success
```

The failing `quality` job reached `cargo xtask verify`; formatting, Clippy, architecture/fixture gates, and the workspace tests preceding the xAI ACP test all progressed successfully. The exact terminal failure was:

```text
test grok_session_run_command_executes_once ... FAILED
thread 'grok_session_run_command_executes_once' (42463) panicked at apps/agent-vesper-acp/tests/xai_native.rs:271:54:
called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound, message: "No such file or directory" }
error: test failed, to rerun pass `-p agent-vesper-acp --test xai_native`
xtask failed: cargo exited with exit status: 101
```

Line 271 reads the isolated-root marker after the provider/tool continuation completes:

```rust
let marker = std::fs::read_to_string(marker).unwrap();
```

The process harness starts the ACP binary in its unique isolated temporary root, sends the same root as the ACP session `cwd`, and `RunCommand` resolves its working directory from the primary tool-context workspace root. The test command writes `command-marker.txt` relative to that root. The failed run therefore reports a missing expected test marker; it does not report a compiler error, disk exhaustion, provider call, production crash, or release artifact failure.

### Comparison with adjacent exact commits

The release commit itself was green in canonical run `36510375890`. Commit `7b04588` changed only the v0.24.4 execution report and evidence index:

```text
7b04588 Close v0.24.4 release evidence
 ...2026-09-29-v0.24.4-combined-corrective-release.md | 125 ++++++++++++++++++++-
 docs/foundation/evidence-index.md                    |   9 +-
 2 files changed, 125 insertions(+), 9 deletions(-)
```

No production or test source changed between the green release commit `d221135` and failed closeout commit `7b04588`.

The other exact-commit workflows on `7b04588` completed successfully:

```text
msrv run 36516876772: success
web-driver run 36516876754: success
five-target-foundation run 36516876732: success
```

### Local reproduction attempts

Disk exhaustion was ruled out before local Rust verification:

```text
/dev/nvme0n1p3  952G  409G  536G  44% /
target: 148G
```

The exact failing test passed 20/20 isolated repetitions:

```text
test grok_session_run_command_executes_once ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out
```

The complete four-test `xai_native` binary then passed 50/50 repetitions (200 individual test cases total):

```text
running 4 tests
....
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
completed 50 full xai_native iterations
```

No local repetition produced a missing marker, command failure, timeout, or process-harness cleanup collision.

## Repair

The repeated local passes did not justify hiding the failure, so the repair removed the marker-location ambiguity while retaining the real exactly-once side effect:

1. Commit `ef84295` changed the command to name the marker under the harness's exact isolated root and added `marker_command_targets_explicit_path`.
2. Push run `36522386399` proved `pull-request-validation` green, but five-target run `36522385949` exposed a Windows-specific defect: Rust's `Command("cmd").arg("/C").arg(command)` and `cmd.exe` do not share normal Windows argv quote parsing, so the quoted absolute marker command did not create the file. The Windows `xai_native` failure remained the same missing-marker assertion.
3. Commit `96cdb48` retained the explicit absolute marker but encoded the Windows PowerShell script as UTF-16LE base64 for `-EncodedCommand`. The `/C` command is now quote-free, while the decoded script appends exactly one `x` to the exact marker path. No sleep, retry, or relaxed assertion was introduced.

Local post-repair commands and receipts:

```text
cargo fmt --all --check
cargo test -p agent-vesper-acp --test xai_native --all-features grok_session_run_command_executes_once -- --exact
cargo test -p agent-vesper-acp --test xai_native --all-features marker_command_targets_explicit_path -- --exact
focused command test: 20/20 passed
full xai_native suite: 20/20 passed (100 cases)
cargo xtask verify
# result: success

git diff --check
# result: success

cargo check -p agent-vesper-acp --test xai_native --all-features --target x86_64-pc-windows-msvc
# not executable from the Linux host: aws-lc-sys selected host `cc` and failed
# before compiling the test because the MSVC cross compiler/NASM were unavailable
```

Exact pushed-commit CI receipts for `96cdb48a00c5fb492cb080e4646c973972a0491e`:

```text
pull-request-validation 36526811056: success
msrv                   36526811116: success
web-driver             36526811114: success
five-target Windows job 109271556643: success
  Run eligible foundational tests: success
  Run Windows prepared conformance: success
```

The overall five-target run `36526811071` was red only because unrelated macOS Intel test `vesper-mcp::session_tests::timeout_is_bounded_quarantined_and_never_replayed` timed out. Linux x86_64, Linux ARM64, macOS Apple Silicon, and Windows all passed; the repaired `xai_native` suite passed on macOS Intel before that unrelated MCP failure.

### Final documentation-closeout exact-commit result

The documentation closeout commit `78b459bcc026be0c981a45e848da27d365362b72` subsequently completed all four push workflow families successfully:

```text
pull-request-validation 36530351592: success
msrv                   36530351579: success
web-driver             36530351551: success
five-target-foundation 36530351536: success
  windows-x86_64:       success
  linux-x86_64:         success
  macos-intel:          success
  linux-arm64:          success
  macos-apple-silicon:  success
```

The successful macOS Intel rerun included the previously timing-sensitive MCP session test. This closes the exact-commit CI observation without rewriting the historical failure on code commit `96cdb48` or claiming a separately demonstrated MCP source correction.

## Finding

The original quality failure was a real nondeterministic test-design defect: its relative side-effect target depended on agreement between the expected harness root and the runtime shell working directory. Naming the absolute target repaired that ambiguity. The first repair then exposed a separate Windows quoting defect, which the quote-free encoded command repaired. Current CI proves both the requested quality lane and the Windows regression lane green on the final code commit.

## Files created or updated

- Updated `apps/agent-vesper-acp/tests/xai_native.rs` with the explicit marker target, Windows quote-safe encoded command, and target-construction regression check.
- Updated `apps/agent-vesper-acp/AGENTS.md` with the durable explicit-marker verification contract.
- Created and then completed `docs/foundation/2026-09-29-post-release-quality-check-investigation.md` as the formal investigation/repair report.
- Linked this report from `docs/foundation/evidence-index.md` and the owning v0.24.4 release execution record.
- Updated `docs/foundation/AGENTS.md` ownership for the evidence record.

No production runtime, workflow, version, tag, release asset, registry manifest, credential, or installed binary was changed.

## Deviations and unresolved items

- The Linux-hosted Windows cross-check was attempted but could not pass the `aws-lc-sys` build because that host lacks the MSVC cross compiler and NASM. The native Windows CI job supplied the authoritative compile-and-execute evidence and passed.
- Code-commit five-target run `36526811071` remains a historical red receipt because of the unrelated macOS Intel MCP startup timeout identified above. The documentation-closeout commit reran the same matrix successfully on all five targets; no MCP source correction is claimed from that later pass.
- No retry, sleep, or weakened exactly-once assertion was added.
- The pre-existing unrelated dirty and untracked workspace files were preserved.

## Readiness effect

The requested `pull-request-validation / quality` failure is repaired: canonical workflow `36526811056` is green on final code commit `96cdb48`, and the Windows lane that caught the first repair's quoting defect is also green. Documentation-closeout commit `78b459b` then passed pull-request validation, MSRV, web-driver, and every five-target foundation job. A later evidence-only commit triggered run `36534993336`, which exposed separate Unix process-group and macOS socket-inheritance races; their diagnosis and repair are owned by [`2026-09-29-red-main-ci-repair.md`](2026-09-29-red-main-ci-repair.md). The tagged v0.24.4 release and Alex's local installation remain unchanged.
