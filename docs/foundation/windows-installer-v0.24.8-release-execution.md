# Windows installer patch-release execution

## Objective and status

Publish the verified Windows PowerShell 5.1 x86_64 installer repair in the next patch, without replacing the developer's installed application. **In progress: no v0.24.8 tag, release, or exact-main prerequisite matrix is claimed.** The reporter's Windows 10 acceptance remains separate from hosted Windows Server 2025 evidence.

## Methods and files

Worked in isolated Git worktree `/tmp/vesper-windows-installer-verification`, branch `verify/windows-ps51-installer-202610`; the primary checkout and its unrelated changes were not reset or staged. The implementation, installation audit, scripts, CI fixture, PRD, documentation and owning DOX are in [the repair report](windows-powershell-installer-repair.md). Addressed the first canonical PR matrix failure in `apps/agent-vesper-tui/tests/voice_execution_policy.rs` by replacing its timestamp/PID temporary workspace name with `tempfile::TempDir`'s unique, owned directory. Updated the repair report and `docs/settings-and-update-prd.md` with native Windows receipts. PR #41 contains the work; release publication must wait for exact-commit canonical, MSRV, five-target foundation, and contained web-driver image acceptance on `main`.

## Exact evidence to date

- Prior PR HEAD `a96713822ff991e4146a4d592b89b74d5466a329`: canonical run `37582397534` Windows installer job emitted `PASS: PowerShell installer offline direct/piped/WOW64, launchers and checksum guards` independently under PowerShell 5.1 Desktop and PowerShell 7+. Standalone native Windows live-probe run `37581588670` installed the published v0.24.7 zip through the repaired script and printed `PASS real release` for both installed ACP/TUI `--version` and `--help` checks. This probes an existing package, not v0.24.8.
- Prior PR HEAD `a9671382`: five-target run `37582397131` **failed** in macos-intel at `apps/agent-vesper-tui/tests/voice_execution_policy.rs:319` (`left: Cpu`, `right: NpuRequired`); four other jobs passed. No exact-release gate credit is assigned.
- Focused local command after fixture change: `cargo test -p agent-vesper-tui --test voice_execution_policy --features 'voice-conversation voice-flm'` returned `12 passed; 0 failed`; `cargo fmt --check` and `git diff --check` exited 0. This does not certify macOS Intel.
- Branch repair commit `997197f7c53367b3a799734b3cea94e2a0935ad4` pushed to PR #41. Its four native PR workflows **completed successfully**: `pull-request-validation 37587967918` (quality, supply-chain, Windows installer), `msrv 37587967932` (rust-1-88), `five-target-foundation 37587967910` (all five jobs), and `web-driver 37587967916` (all three jobs). The macos-intel log at `2026-10-07T07:42:02Z` printed `test copied_strict_config_is_locally_revalidated_and_explained_not_rewritten ... ok`. These PR runs are not main release prerequisites.
- A later local `cargo xtask verify` **failed** during concurrent test compilation with `rustc-LLVM ERROR: IO failure on output stream: Disk quota exceeded` on the 14-GiB `/tmp` tmpfs; log `/tmp/vesper-installer-branch-xtask-verify.log`. This is not a passing local verification. Removed only this isolated worktree's `target/debug/incremental` (3.1 GiB) to restore free space; did not delete user-owned builds. Hosted PR quality job passed under run `37587967918`; this does not turn the failed local run into a pass.

## Deviations, unresolved items and readiness effect

The dedicated live probe was removed after it passed; it is not a permanent CI gate. The macOS failure's temporary-directory collision is a plausible fixture diagnosis given the differing on-disk/read-back policy and timestamp-generated paths, not independently observed file-level proof. Full main-commit gates, version/tag/publication, downloaded new-package smoke, Registry update if required, public raw-main one-liner, the reporter's Windows 10 install and interactive TUI acceptance are pending. No installer was run on Alex's installation. Remain release-blocked while any required gate is red or pending; preserve exact failures and re-evaluate against the changed source.
