# Tool-Output Stall Five-Target Platform Verification — 2026-09-25

## Objective

Close the release-relevant Windows and macOS behavioral gap for the accepted
classification-D shared executor repair. Use a dedicated temporary ref only;
do not version, tag, release, install or merge unrelated work.

## Candidate and Method

- Verification ref: `verify/tool-output-settlement-20260925`
- Exact behavior candidate: `27a3f17bd818294e1a52e0bf194b02ba761e3464`
- Final workflow: `five-target-foundation` run `36036088105`
- Workflow URL: <https://github.com/99percentgrip/agent-vesper/actions/runs/36036088105>
- Native targets: Linux x86_64, Linux ARM64, Windows x86_64, macOS Intel and
  macOS Apple Silicon.

Every target ran `cargo test --workspace --all-features` and then the explicit
single-threaded `command_settlement` matrix. The nine cases cover large stdout,
large stderr, alternating mixed streams, shared-cap truncation while continuing
to drain, exact retained-output boundaries, the historical 71 KiB and larger
stress shapes, timeout, signal cancellation, caller/task abortion, nonzero exit,
descendant-held pipes, descendants writing after leader exit, owned-tree cleanup
and subsequent-command recovery.

## Red-to-Green Platform Evidence

Initial run `36033634456` at `12ebaf989268d821d4b2fe02822555ca161c4bd6`
proved that the first portable fixtures were invalid on Windows: five of nine
cases failed because quoted PowerShell was reinterpreted by `cmd /C`, producing
short error output or the wrong exit code. Linux ARM64 also encountered an
unrelated transient `vesper-voice` executable-busy test failure. Both obsolete
runs were retained as evidence and then cancelled after their terminal logs were
captured.

Candidate `9d30a7f2cf951fb0fb7683b86e5f49091575c007` changed Windows fixtures to
UTF-16LE PowerShell `-EncodedCommand` payloads. Run `36035098957` flipped seven
of nine Windows cases green. The remaining two failures showed startup-race
assertions: cancellation could fire before partial output appeared, and caller
abort allowed only two seconds for PowerShell startup.

Candidate `27a3f17bd818294e1a52e0bf194b02ba761e3464` replaced those sleeps with
explicit readiness markers and a bounded five-second fixture-start budget. Local
Linux verification remained 9/9 green; strict affected-package Clippy, formatting
and whitespace checks passed. Exact-candidate run `36036088105` then passed all
five jobs and every dedicated settlement step.

## Exact Results

| Target | Job ID | Dedicated settlement | Full job |
|---|---:|---|---|
| Linux x86_64 | `107756425168` | PASS, 18:00:07–18:00:22 UTC | PASS |
| Linux ARM64 | `107756425250` | PASS, 18:01:37–18:01:52 UTC | PASS |
| Windows x86_64 | `107756425081` | PASS, 18:19:41–18:20:00 UTC | PASS |
| macOS Apple Silicon | `107756424741` | PASS, 18:01:53–18:02:09 UTC | PASS |
| macOS Intel | `107756425176` | PASS, 18:30:22–18:30:39 UTC | PASS |

The native Windows Job Object and POSIX process-group implementations therefore
pass descendant-held-pipe and descendant-cleanup behavior on every supported
target family. Compilation alone is not used as evidence.

## Provider-Correlation Boundary

Provider correlation is **NOT EXECUTED — GLM live access unavailable**. Alex no
longer has an active GLM subscription/access and does not authorize restoring or
purchasing access for this diagnostic. Preserve the historical observation that
the stall appeared much more frequently with OpenAI, but infer no frequency
explanation.

Fixture-backed OpenAI/GLM tests prove provider-visible tool-surface parity and
tool-call protocol handling. The provider-neutral production executor reproduced
red and the shared repair is green. Those receipts establish classification D;
they are not a matched live-provider comparison. The unavailable comparison is
an unresolved explanatory limitation, not a blocker to release of this repair.

## Provenance and Scope

The old installed incident binary cannot be mapped byte-for-byte to the current
source and remains a historical provenance limitation. It does not block release:
current source independently reproduced the defect at the real `RunCommand`
boundary and the repaired exact source passed native five-target behavior.

The primary dirty checkout at `/home/Alex/Projects/agent-vesper` was not changed.
All implementation and CI work stayed in the isolated worktree and temporary
verification ref. No version bump, tag, release, installation or unrelated merge
was performed.

## Changed Files

- Shared executor and regression files recorded in the permanent-repair report.
- `.github/workflows/platform-foundation.yml` for the explicit five-target
  behavioral step.
- Windows-only test fixture encoding and readiness synchronization.
- This report, the authoritative incident ledger and evidence index.

## Verification Commands and Receipts

- `cargo test -p vesper-agent --test command_settlement -- --test-threads=1`:
  local PASS, 9/9 after each fixture correction.
- `cargo clippy -p vesper-agent --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS.
- GitHub Actions run `36036088105`: PASS, five of five native jobs; exact head
  `27a3f17bd818294e1a52e0bf194b02ba761e3464`.

## Readiness Effect

Windows, macOS Intel and macOS Apple Silicon behavioral acceptance is closed.
The shared executor repair is PASS and the critical tool-output stall is PASS at
the approved scope. The candidate is ready for release preparation. Release work
was not started in this unit.
