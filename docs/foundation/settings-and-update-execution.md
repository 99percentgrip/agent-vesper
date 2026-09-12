# Native Settings and update repair execution

Date: 2026-09-12. Baseline: `e8827aca8618f26bbc22eaa583eb5e77f410484b`.
Scope: [owning PRD](../settings-and-update-prd.md), S1–S5.
Status: implemented and verified on Linux; Windows native update acceptance open.
Implementation evidence below precedes release preparation. The authorized
[0.22.2 release record](v0.22.2-release-execution.md) tracks publication;
the user will perform the local installation test.

## Objective and methods

Repair the screenshot-reported grayscale menus, fragmented saving, mandatory
manual PRD path, and check-only update action. Inspected host composition,
shared acceptance enforcement, adapter execution and shipped installers; used
buffer assertions, real terminal interaction, scripted AgentLoop providers,
loopback HTTP and isolated installer payloads. No live provider requests,
public update downloads or real user-state writes were used for verification.

## Changes and requirement evidence

| Requirement | Implementation and current evidence |
| --- | --- |
| S1 | Providers, Swarm and acceptance use the shared themed renderer. Six-theme buffer tests compare canvas/selection styling; real TUI terminal flow passes. |
| S2 | Central Settings clones a draft and offers Save changes, Discard changes or Keep editing. Mouse/keyboard PTY covers all outcomes and provider cancellation. Grouped-save regression proves exact-byte rollback and removal of newly created files on ordinary failure. |
| S3 | Save persists validated ordinary choices, partitioning provider-specific values. Restore and PTY cover model, thinking, auxiliary model, generation, mixture, permission and session mode, with active Bypass and restart checks. Local-model window regression refuses absent catalog metadata. Both actual LM Studio HTTP adapters now send the requested model instead of the launch model; both loopback wire regressions pass. |
| S4 | Native opt-in without a path creates pending, incomplete acceptance. Both hosts capture the originating user request outside model-controlled history. The permission-gated enrollment tool checks a recognized PRD against that request through independent source inspection, prepares the contract and remembers its path. Real AgentLoop enrollment/repair/verification regression proves enrollment alone cannot complete; restart requires fresh evidence. Missing/external/forged scope, independent refusal and cancellation are rejected without remembering a path. Native ACP pending activation makes zero provider requests. All 23 exact acceptance cases pass. |
| S5 | A newer stable release offers Install update / Not now with destination and restart guidance. Confirmation runs the embedded shipped installer with an exact version and no release-base override. Linux displays real installer log/status; Windows hands off to a console that waits for the host to exit. Command/version tests and a full offline POSIX installer fixture prove bad checksums preserve the old payload and valid checksums install while preserving user state. Native Windows handoff is not verified. |

Automatic enrollment follows [ADR 0029](../adr/0029-automatic-prd-enrollment.md).
It does not grant permission, replace an enrolled scope, invent evidence or
relax the existing Rust/platform verification limits. Ambiguous scope can still
require a user clarification. Offline scripted tests do not measure live-model
PRD recognition effectiveness.

## Files

- `apps/agent-vesper-tui/src/settings_host.rs`, `settings_menu.rs`,
  `provider_hub.rs`, `swarm_hub.rs`, `acceptance_host.rs`, `main.rs`,
  `dispatch.rs`: draft navigation, themed controls, persistence and execution.
- TUI `src/update_host.rs`, `landing_host.rs`, `Cargo.toml`: confirmed installer,
  version handling, progress and temporary artifacts.
- Both hosts' `src/lmstudio_provider.rs`, ACP `src/lib.rs`, both
  `src/swarm_host.rs`, ACP `tests/acceptance_controls.rs`: selected-model
  execution and cross-host automatic enrollment.
- `crates/vesper-harness/src/acceptance.rs`, `acceptance_settings.rs`,
  `acceptance_tests.rs`; `xtask/src/main.rs`: protected enrollment and mandatory
  acceptance regressions.
- TUI `tests/settings_pty.py`, `update_curl_fixture.sh`,
  `update_payload_fixture.sh`, `AGENTS.md`: isolated terminal/installer evidence.
- Root, TUI, ACP, harness, xtask and documentation `AGENTS.md` contracts;
  `docs/settings-and-update-prd.md`, ADR 0029, user/install/web guides,
  completion-assurance evidence cross-link and foundation evidence index.

## Commands and exact results

Commands ran from the repository root on Linux, rustc 1.95.0 unless specified.
Logs are local diagnostic artifacts under `/tmp`, not durable release evidence.

| Command | Result / local log |
| --- | --- |
| `cargo test --workspace --all-features --offline` | Final: **2,234 passed, 0 failed, 34 ignored**; `/tmp/vesper-settings-workspace-final.log`. |
| `cargo +1.88.0 test -p agent-vesper-tui -p agent-vesper-acp -p vesper-harness --lib --bins --all-features --offline` | Final: **548 passed, 0 failed, 4 ignored**; `/tmp/vesper-settings-msrv-final.log`. |
| `cargo xtask acceptance` | Final: **23 exact cases passed**, 6,381 ms; `/tmp/vesper-settings-gate.log`. |
| `cargo xtask verify` | Passed architecture, naming, fixtures, default tests and acceptance; `/tmp/vesper-settings-verify.log`. Ran before final UI/installer fixture refinements; final affected-code coverage comes from the above reruns and Clippy below. |
| `cargo clippy -p agent-vesper-tui -p agent-vesper-acp -p vesper-harness --all-targets --all-features --offline -- -D warnings` | Final clean; `/tmp/vesper-settings-clippy-final.log`. |
| `cargo build -p agent-vesper-tui --all-features --offline` | Final debug binary built; `/tmp/vesper-settings-build-final.log`. |
| `python3 apps/agent-vesper-tui/tests/settings_pty.py target/debug/agent-vesper-tui` | Final PASS: mouse navigation, discard, keep editing, grouped save, pending PRD toggle, active permission and restart persistence; `/tmp/vesper-settings-pty-final.log`. |
| `sh scripts/test_install_upgrade.sh` | PASS: payload replacement preserves cognition contents/inodes, voice state and unrelated files. |
| `cargo check -p agent-vesper-tui --all-features --target x86_64-pc-windows-msvc --offline` | BLOCKED by Linux native cross-build dependencies (`libsqlite3-sys`, `aws-lc-sys` C toolchain failures); `/tmp/vesper-settings-windows.log`. Does not establish Windows compilation or execution. |
| `cargo fmt --all -- --check`; `git diff --check` | Clean at closeout. |

The updater fixture
`updater_installs_verified_fixture_and_refuses_bad_checksum_without_touching_user_state`
passes in the final workspace and MSRV runs. It executes the shipped POSIX
installer against a local archive/checksum supplied by an immutable fake curl;
all home, install, temporary and workspace roots are isolated.
Initial sandbox runs rejected loopback sockets; permitted reruns passed.
Development-only compile/fixture failures were corrected before final reruns;
none are counted as successful evidence. Existing ignored tests remain ignored.

## Deviations and unresolved items

- Fixed LM Studio transport and selected-model context limits discovered while
  tracing whether saved model choices actually reach execution, in both hosts.
- Provider authentication and driver import remain explicit immediate setup
  effects. Discard cannot undo imported images or saved credentials.
- Ordinary grouped writes roll back reported failures; this is not a multi-file
  crash-atomic database transaction.
- Windows native PowerShell console handoff, binary replacement and failure UI
  still need native platform acceptance. macOS updater execution was not run.
  No public GitHub download/install or live-provider effectiveness trial ran.
- The existing installed executable has not been replaced. The built debug
  binary contains these changes. No release workflow or exact-commit platform
  gates were run; this report does not authorize a release-complete claim.

## DOX and readiness

Rechecked changed paths against the root and nearest contracts. Updated root
preferences and owning TUI/ACP/harness/xtask/docs contracts, added the TUI tests
child boundary/index, and linked this report from its PRD and evidence index.
Clarified grouped versus standalone Web saving in the guide and TUI contract.
Parent `apps/AGENTS.md`, `crates/AGENTS.md`, and `scripts/AGENTS.md` intentionally
remain unchanged: their ownership/dependency/installer contracts still hold;
installer sources were reused without alteration. ADR 0028 remains immutable;
ADR 0029 explicitly records its enrollment refinement.

Linux source behavior is ready for review and local use. Cross-platform update
acceptance remains open and must be completed before claiming S5 fully verified
on all supported platforms or publishing an exact-commit-gated release.
