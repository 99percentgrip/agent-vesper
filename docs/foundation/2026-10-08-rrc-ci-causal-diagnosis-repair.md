# RRC settled-CI causal diagnosis repair

## Objective and status

Repair the native v0.24.9 controller's observed evidence stop and the three
settled CI failures. Provider-neutral shared diagnosis remains subject to focused
proof, existing permissions, exact-commit gates and persisted retry bounds.
Implementation and scoped local proof are recorded below; release completion,
actual Windows installer acceptance and Alex's laptop retest remain pending.

## Incident and exact evidence

Candidate `c076ebc7bad1fb39c1675ec4ddcecf75d0e44c1f` completed all eight native
local gates. Its four public prerequisite workflows fully settled: nine jobs
passed and three failed. Canonical 37669161700, MSRV 37669161803, five-target
37669161802 and web-driver 37669161667 are the exact-source receipts. Windows,
Apple Silicon, Intel macOS and Linux ARM64 foundation jobs passed. No tag or
publication occurred.

The native owner waited for the complete matrix, captured all failures, then
stopped at NeedMoreEvidence with no repair admission or retry spend. This is
preserved in the [native stopped record](2026-10-08-rrc-ci-causal-diagnosis-repair-native-stopped.json).
It is not autonomous repair success. The temporary owner exited via /quit only
after the worker was idle; Alex's original TUI and installation were untouched.

- Windows installer job 112956101737 compiled the actual shipping executables
  and passed the CRT import audit. Both offline PowerShell host tests passed.
  The actual installer succeeded under Desktop 5.1, but its subsequent check
  captured stdout alone. ACP intentionally writes metadata to stderr, so the
  nonempty-output assertion failed. PowerShell 7 real acceptance was skipped.
  RRC instead selected normal Runner Image Provisioner metadata and called it
  infrastructure failure.
- Linux foundation job 112956102164 failed to fetch the Bubblewrap package over
  HTTP, then panicked because Bubblewrap was unavailable. RRC missed the explicit
  earlier E: Failed to fetch diagnostic and selected the downstream test panic.
- Browser job 112956101712 timed out during APT dependency acquisition, before
  the browser test. RRC retained the final timeout alone and discarded the
  observed Installing dependencies, mirrorlist and ignored-fetch context.

No official service outage or billing problem is established by these logs.

## Changes and methods

- Shared causal matching recognizes CLI version/help assertions and explicit
  APT fetch failures. Classification follows the selected first diagnostic so
  a later missing-binary panic cannot replace a download cause. Normal runner
  provisioning text no longer proves an infrastructure failure.
- A timed-out package setup retains at most four observed setup/fetch lines
  from a bounded same-step window. Command echoes, earlier steps and generic
  timeouts remain uncertain. No uncertainty or permission guard is relaxed.
- Windows real-package checks capture both streams, tolerate Desktop 5.1's
  NativeCommandError wrapping and use actual native exit codes. No failing
  process is converted into success.
- Shared CI-only APT preparation preserves cached mirror+file references,
  rewrites known Ubuntu HTTP sources to architecture-correct canonical HTTPS
  (ARM64 retains the ports archive), and configures IPv4,
  bounded retries and both transport timeouts. Sandbox retains outer 120-second
  bounds; browser setup adds an outer 240-second bound. Tests are retained.
  Acquisition options are documented by the [Ubuntu APT manual](https://manpages.ubuntu.com/manpages/noble/man5/apt.conf.5.html).

## Verification

- Three named causal regressions: pre-change red, 0 passed / 3 failed.
- `cargo test -p vesper-harness --lib --all-features release_recovery::tests:: -- --nocapture`:
  100 passed / 0 failed, 139.70 seconds. Includes new causal cases and existing
  unknown, cancellation, account restriction, fingerprint and permission guards.
- `python3 .github/test_prepare_linux_apt.py`: 4 passed; private fixtures only.
- `python3 scripts/test_windows_package_audit.py`: 7 passed, including both-stream
  and native-exit policy assertions.
- `python3 scripts/test_release_gate.py`: 10 passed.
- Three changed workflow YAML files parsed successfully.
- Three causal regressions are enrolled in mandatory xtask acceptance.
  `cargo xtask acceptance`: 135/135 passed in 222643 ms.
- Strict all-target/all-feature harness Clippy with `-D warnings`: passed, 46.40 s.
- Default native TUI build: passed, 37.51 s. Formatting and whitespace passed.
- Fresh hosted reruns, publication and final native receipt remain required.

[Raw receipts](2026-10-08-rrc-ci-causal-diagnosis-repair-receipts.tar.gz) retain
all three complete failed-job logs, red/green diagnosis and scoped script checks.
[Source and receipt manifest](2026-10-08-rrc-ci-causal-diagnosis-repair-evidence.json) binds code and raw-log hashes.
Constructed regression excerpts are grounded in those logs; they do not measure
live-model coding repair effectiveness.

## Files, deviations and DOX

Changed shared harness diagnosis, mandatory xtask inventory, real-package Windows
check and its policy test, CI-only APT helper/tests and all three affected workflows.
Updated nearest harness, xtask, scripts and GitHub contracts; this report is linked
from the owning RRC PRD and evidence index. Root, crates and documentation parents
retain existing ownership: no new subtree or additional permission is introduced.
The private source branch advanced to the already pushed native version candidate;
no manual version edit, tag, installation or publication was performed.

## Unresolved items and readiness effect

The observed diagnosis defects and concrete CI inputs are repaired locally.
Fresh full native gates and all complete hosted matrices for final source remain
required, followed by producing assets, Registry update, native final receipt and
summary. This report preserves the failed v0.24.9 candidate instead of treating
old green tests as proof of current completion. Generic unknowable failures still
stop safely; universal bug-free RRC parity is not certified. Windows laptop
acceptance cannot be fabricated by a hosted runner.
