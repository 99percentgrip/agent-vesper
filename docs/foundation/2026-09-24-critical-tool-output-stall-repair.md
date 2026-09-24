# Critical Tool-Output Stall Permanent Repair — 2026-09-24

> **Superseded closeout status (2026-09-25):** exact candidate
> `27a3f17bd818294e1a52e0bf194b02ba761e3464` passed the five-target native
> behavioral workflow (`36036088105`), including Windows and both macOS lanes.
> Per Alex's corrected acceptance scope, provider correlation is **NOT EXECUTED —
> GLM live access unavailable** and is an explanatory limitation rather than a
> blocker to release of the independently proven classification-D repair. The
> earlier OPEN statements below describe this report's pre-CI checkpoint.

## Objective

Permanently repair the recurring `run_command` stall under large stdout/stderr,
truncation, timeout, cancellation and descendant-held pipes. Prove the shared
executor boundary before changing it, keep provider protocol concerns separate,
exercise both production host compositions, and preserve truthful failure state
without replaying an ambiguous command.

## Baseline and Provenance

- Work was isolated in `/tmp/agent-vesper-tool-output-stall`; Alex's dirty primary
  checkout was not reset, stashed, cleaned, overwritten or used for edits.
- Source baseline: detached `v0.23.5` commit
  `01461a32fea270aaa4e046cefbd6b31b4cc7c7e2`.
- Installed wrappers resolve to
  `/home/Alex/.local/share/agent-vesper/agent-vesper-acp` and the corresponding
  TUI binary. Both installed binaries report `0.23.3`, while the source baseline
  is `0.23.5`; the incident executable cannot be proven byte-identical to this
  source.
- Installed SHA-256: ACP
  `de7cb691eb47764e2fdc39b044b243b766ffc023f468868178be20f06181e644`;
  TUI `c4f29171eaad90cefc7e01af6f12f5a3cc47031bc71b558606243dcd0e07a11f`.
- No installed binary, release, tag, remote branch or user state was changed.

## Reconnaissance and Root Cause

The shared registry advertises the same nine core definitions in code mode:
`apply_patch`, `edit_file`, `grep`, `list_directory`, `read_file`,
`run_command`, `search_files`, `update_plan`, and `write_file`. The common agent
loop performs mode/deferred-tool filtering before either adapter sees the
`ProviderRequest`. Adapter regressions prove that OpenAI API-key and ChatGPT
authentication serialize the same ordered names, descriptions and schemas with
explicit automatic tool choice, while GLM serializes the same surface and uses
its documented default-auto omission. Existing round-trip tests prove tool-call
identity, argument assembly, result continuation, terminal events and ambiguous
fragment handling for both adapters.

No matched live-provider calls were made. Historical evidence has no denominator
or matched prompt series, so Alex's observation that OpenAI selects the shell path
more often remains a credible exposure hypothesis, not a proved selection-rate or
provider defect.

The direct provider-neutral reproduction proved classification **D — shared
executor defect**. The old `run_bounded` implementation spawned piped stdout and
stderr, polled `try_wait` until process exit, and called `wait_with_output` only
afterward. A child writing 256 KiB to stdout filled the kernel pipe and blocked;
the parent waited for the blocked child to exit. The red regression returned
`command timed out and was killed` after exactly 2.00 seconds. Output limiting
occurred only after full collection, so it neither bounded collection memory nor
kept transport moving.

TUI and ACP both compose the same `vesper-agent::tools::RunCommand` executor for
the unsandboxed route. The sandbox route is distinct: its namespace supervisor
already drains stdout/stderr concurrently and owns descendant cleanup.

## Acceptance Checklist

1. **Capture identity and red-first reproduction — PASS.** The production shared
   `RunCommand` boundary failed on a 256 KiB stdout fixture before repair.
2. **Concurrent draining and bounded retention — PASS on Linux.** Both readers
   start immediately, share a 65,536-byte retained budget and continue counting
   drained bytes after retention fills.
3. **Timeout/cancellation and descendants — PASS on Linux.** The command owns a
   new process group; cancellation, task drop, timeout and leader completion all
   trigger bounded group cleanup, reap and reader settlement.
4. **Truthful output/outcome — PASS.** Results distinguish success, nonzero exit,
   timeout, cancellation, reader failure and cleanup uncertainty; partial output,
   known status, drained/retained counts and truncation survive within the cap.
5. **Shared executor and hosts — PASS locally on Linux.** Shared, TUI, ACP and
   sandbox tests pass. Windows and macOS behavior was not executed.
6. **Provider correlation — OPEN / NOT EXECUTED.** Fixture-backed surface and
   protocol parity pass; matched live provider selection frequency needs explicit
   live credentials/network authorization and remains outside foundation tests.

## Implementation

- Start independent stdout and stderr drain threads immediately after spawn.
- Separate transport from retention with one atomic 65,536-byte budget. Readers
  continue to EOF after their retained share reaches zero and keep total-byte and
  truncation accounting.
- Use `command-group` 5.0.1 (MSRV 1.68) to own a POSIX process group on Unix
  and a Job Object with kill-on-close on Windows. Cleanup targets that retained
  OS object even after the shell leader exits; it does not rediscover descendants
  from an exited leader PID.
- Treat leader status, group cleanup, reap and both pipe EOFs as independent
  settlement evidence with bounded finalization windows.
- Add a drop guard so aborting the async caller signals the blocking supervisor;
  dropping a `spawn_blocking` future no longer abandons its process tree.
- Return a failed `ToolError` for timeout, cancellation, nonzero status, read
  failure, or uncertain cleanup. Preserve bounded partial stdout/stderr and
  drained/retained metadata. No replay path was added.
- Correct the generic output bound so its truncation marker is included inside
  the 65,536-byte maximum.

## Reuse Audit

- Existing `vesper-agent` command executor: **UNSAFE/REPLACED IN PLACE**. The
  repair changes its only production primitive rather than adding another host
  implementation.
- `vesper-harness::acceptance_runner`: **KEEP SEPARATE WITH JUSTIFICATION**. It
  has verifier-specific environment and fail-on-truncation behavior, and the
  dependency direction does not permit `vesper-agent` to import it. Its
  concurrent-drain/process-group pattern informed the repair.
- `vesper-sandbox` namespace supervisor: **KEEP SEPARATE WITH JUSTIFICATION**.
  PID-namespace and `PDEATHSIG` ownership belong to its isolation backend; it
  already drains concurrently.
- `spikes/process-sandbox`: **UNSAFE/INCOMPATIBLE FOR PRODUCTION REUSE**. It is a
  disposable evidence package and cannot become a production dependency.

## Changed Files

- `crates/vesper-agent/src/tools.rs`
- `crates/vesper-agent/tests/executors.rs`
- `crates/vesper-agent/tests/command_settlement.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-acp/src/lib.rs`
- `crates/vesper-provider-openai/src/tests.rs`
- `crates/vesper-provider-glm/src/request.rs`
- `.github/workflows/platform-foundation.yml`
- workspace and `vesper-agent` Cargo manifests plus `Cargo.lock`
- nearest owning `AGENTS.md` files for the changed source/tests
- the authoritative incident record, this report, and
  `docs/foundation/evidence-index.md`

## Red-to-Green Evidence

- **Red:** pre-repair `large_stdout_over_pipe_capacity_settles` failed after
  exactly 2.00 s with `command timed out and was killed` for 256 KiB stdout.
- **Green:** `command_settlement` passed 9/9 in 1.03 s. It covers 1 MiB stdout,
  1 MiB stderr, alternating 512 KiB + 512 KiB streams, 65,534/65,536/65,537-byte
  boundaries, the historical 71,443-byte instruction file, deterministic
  `git diff --no-index`, nonzero exit, timeout, cancellation, dropped caller,
  leader-exited/descendant-held pipe, continued descendant writing and a next
  successful command.
- Retained output never exceeds 65,536 bytes. The over-boundary fixture records
  `stdout drained=65537`; pressure fixtures record 524,288 or 1,048,576 drained
  bytes while the retained result stays bounded.
- Linux `/proc` checks prove recorded timeout and leader-exit descendants are gone
  within two seconds. Every pressure/held-pipe case settles under three seconds;
  timeout settlement stays under four seconds.
- Injected reader failure preserves the seven already-read bytes and records the
  read error before returning failure.
- TUI and ACP production-registry tests each pass 1/1 and cover large mixed
  output, truncation, timeout, cancellation, descendant-held pipe and recovery.
- A 2026-09-25 portability audit found that the first regression file was
  globally `cfg(unix)` and its descendant-absence assertions were Linux-only.
  The matrix now compiles on every target, uses platform-specific shell fixtures,
  and checks portable delayed descendant markers after timeout, cancellation,
  caller abort and leader exit. Linux passes 9/9 with the revised matrix.

## Commands and Results

```text
cargo test -p vesper-agent --test command_settlement
  PASS — 9 passed, 0 failed

cargo test -p vesper-agent pipe_reader_preserves_partial_bytes_when_reading_fails
  PASS — 1 passed, 0 failed

cargo test -p agent-vesper-tui tui_host_registry_settles_large_command_output_and_recovers
  PASS — 1 passed, 0 failed

cargo test -p agent-vesper-acp acp_host_registry_settles_large_command_output_and_recovers
  PASS — 1 passed, 0 failed

cargo test -p vesper-provider-openai complete_shared_tool_surface_serializes_identically_in_both_auth_modes
  PASS — 1 passed, 0 failed

cargo test -p vesper-provider-glm complete_shared_tool_surface_serializes_without_filtering_or_renaming
  PASS — 1 passed, 0 failed

cargo test -p vesper-sandbox
  PASS — 11 library tests and 6 nonignored namespace tests

VESPER_SANDBOX_INIT="$PWD/target/debug/sandbox_init" cargo test -p vesper-sandbox --test namespaces namespace_security_and_timeout_acceptance -- --ignored --nocapture
  PASS — 1 passed, 0 failed

cargo xtask architecture
  PASS — 30 packages

cargo xtask naming-guard
  PASS

cargo xtask acceptance
  PASS — 23 exact cases

cargo xtask msrv
  PASS — Rust 1.88 workspace all-features tests and doctests

cargo xtask verify
  PASS — formatting, strict all-features Clippy, workspace tests and doctests,
  76 scenarios, 154 fixtures, architecture, naming, provider/runtime/ACP/session/
  testkit gates, and 23/23 acceptance cases

cargo deny check
  PASS — advisories, bans, licenses and sources

cargo audit
  PASS — 496 dependencies scanned; no vulnerabilities reported

cargo check -p vesper-agent --target x86_64-pc-windows-msvc
cargo check -p vesper-agent --target x86_64-apple-darwin
cargo check -p vesper-agent --target aarch64-apple-darwin
  PASS — compile checks only; behavioral cleanup remains NOT RUN

cargo +1.88.0 check -p vesper-agent --all-targets
cargo clippy -p vesper-agent --all-targets -- -D warnings
  PASS — new `command-group` ownership path and portable matrix

cargo fmt --all --check
git diff --check
added-link validation
  PASS — formatting, whitespace and the added relative report link
```

The authoritative incident record was copied from Alex's untracked primary
checkout into the isolated candidate so its status could link this repair. Its
pre-existing links to other untracked historical records cannot resolve inside
the detached worktree; both links added by this repair resolve. The primary
copies and their companion records were not modified.

The heavy local commands used `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0` because `/tmp` is a
bounded tmpfs. Initial acceptance and canonical-verification attempts exhausted
that generated build space; `cargo clean` removed only this isolated worktree's
generated `target/`, and both gates then passed using a dedicated cache target.
No source result was waived.

## Platform and Route Matrix

| Lane | Result | Evidence / limit |
|---|---|---|
| Linux unsandboxed shared executor | PASS | Full behavioral matrix and `/proc` descendant checks |
| Linux TUI | PASS | Real hosted registry test |
| Linux ACP | PASS | Real hosted registry test |
| Linux sandbox | PASS | Package suite plus explicit ignored namespace acceptance |
| Windows | NOT RUN | Portable behavior step prepared; GitHub-hosted execution requires a remote ref, prohibited by the current no-commit/no-push instruction |
| macOS Intel | NOT RUN | Portable behavior step prepared; GitHub-hosted execution requires a remote ref |
| macOS Apple silicon | NOT RUN | Portable behavior step prepared; GitHub-hosted execution requires a remote ref |

## Deviations and Unresolved Items

- No live OpenAI or GLM provider call was made, as foundation verification forbids
  live provider calls and user-state writes. The authoritative incident ledger
  explicitly lists matched provider correlation as patch-acceptance item 6, so
  it remains **OPEN / NOT EXECUTED pending Alex authorization**. It is not needed
  to prove classification D or the provider-neutral repair: serialized tool
  surface/protocol parity and direct shared-executor reproduction already do so.
- Installed/source identity is partial because the installed 0.23.3 binaries do
  not correspond to the 0.23.5 source baseline and historical incident binaries
  were not retained with a source digest.
- Windows and macOS cleanup behavior is not certified by cross-compilation.
  The candidate now uses a Windows Job Object/POSIX process group and has a
  portable behavioral matrix wired into every five-target job, but GitHub Actions
  can only check out a committed remote ref. The user prohibited commit/push for
  this work unit, so no hosted run was launched.
- The current `ToolError` public type remains prose-backed, but the executor now
  emits stable explicit fields in its bounded failure text. No wider typed-contract
  migration was necessary for host behavior.

## Readiness Effect

The provider-neutral shared executor repair is locally accepted on Linux and both
production host compositions recover after every tested pressure/failure path.
Installed/source mismatch is historical provenance only: the current source defect
was independently reproduced red at the production `RunCommand` boundary and the
same source path is green after repair, so byte identity with the old installed
incident binary is not needed for release of the repaired current source.
The authoritative critical incident should remain **OPEN** until its separately
required live provider-correlation box is executed or explicitly re-scoped, and
until required non-Linux behavioral lanes run. The shared executor defect itself
is repaired and green at the tested scope.
