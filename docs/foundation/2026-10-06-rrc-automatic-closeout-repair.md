# RRC automatic closeout and final delivery repair

## Objective and status

Repair the published-release continuation and final reporting failure observed in Alex’s v0.24.6 run. Preserve that version, tag, assets, existing registry PR and retry budget. Base: `a56f0ba76525bf4e7e288eefe2dff731cb120914`; isolated branch: `repair/rrc-automatic-closeout`.

The implementation is locally verified, and the existing v0.24.6 release has completed native closeout. This report does not certify full RRC PRD parity. The repair itself has not been pushed, released or installed.

## Diagnosis and changes

1. The production worker’s terminal predicate included `Published`, although the controller’s next directive was post-release closeout. The published advance arm did nothing. The worker now continues automatically through current-main exact-SHA settlement.
2. Admission asked a release-choice question for all irreversible states, including the same published objective/version. Matching unfinished publication now resumes the existing epoch; mismatched objectives remain protected. A matching completed target returns its existing receipt.
3. Post-release closeout had no native registry/report delivery port. The new shared port validates the committed manifest, published archives and digests, existing open registry PR and its fork/branch. It updates only `agent-vesper/agent.json` with the expected blob, then reads it back. Closed/moved PRs, missing assets and concurrent changes refuse. A journaled uncertain write is reobserved without replay.
4. ACP returned an admission acknowledgement and ended its request before completion, allowing the caller to terminate its owner. Admitted prompts now retain the owner, stream persisted milestones and explicitly emit the final receipt. Concurrent status/cancellation remain available; dead-owner and blocked states report unfinished work.
5. TUI excluded complete RUN snapshots and had no final receipt projection. It now reads the durable completion port after RUN disappears, using a separate completion cursor so the last milestone cannot suppress the final message.
6. Report evidence retains valid structured JSON beyond the diagnostic excerpt limit; restart excludes ephemeral heartbeat/journal fields and preserves edited reports.

## Methods, commands and exact evidence

Foundation checks use isolated temporary repositories, synthetic providers and controlled commands. No live provider call or real credential access is part of those checks. Cargo gates run sequentially with `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1` and the existing candidate target cache.

| Check | Receipt |
| --- | --- |
| Pre-change worker regression | `published_release_keeps_worker_until_post_release_closeout`: 1 executed failure; retained `/tmp/vesper-rrc-closeout-red.log`, exit 101 |
| Focused closeout suite | `cargo test -p vesper-harness --lib --all-features closeout -- --nocapture`: 10 passed, 0 failed; `/tmp/vesper-closeout-tests-final.log` |
| Shared controller suite before final additions | `cargo test -p vesper-harness --lib --all-features`: 359 passed, 0 failed, 6 ignored; `/tmp/vesper-closeout-harness-all.log` |
| ACP real process | `cargo test -p agent-vesper-acp --test release_resource_governor --features integration-test-harness -- --test-threads=1`: 4 passed, 0 failed; `/tmp/vesper-closeout-acp-tests.log` |
| TUI binary suite before final cursor regression | `cargo test -p agent-vesper-tui --bin agent-vesper-tui --all-features`: 170 passed, 0 failed, 1 ignored; `/tmp/vesper-closeout-tui-tests.log` |
| TUI final receipt regression | 1 passed, 0 failed; `/tmp/vesper-closeout-tui-final.log` |
| Clippy | `cargo clippy -p vesper-harness -p agent-vesper-acp -p agent-vesper-tui --all-targets --all-features -- -D warnings`: passed; `/tmp/vesper-closeout-clippy.log` |

GitHub registry writes follow the official [Contents update contract](https://docs.github.com/en/rest/repos/contents#create-or-update-file-contents): encoded content, expected existing blob and explicit branch, followed by independent observation. No PR comment/message, PR replacement, merge or force push is part of closeout.

## Files and DOX pass

- Shared implementation: `crates/vesper-harness/src/release_closeout.rs`, `release_executor.rs`, `release_recovery.rs`, `lib.rs`, crate manifest and lockfile.
- Host delivery: ACP `src/lib.rs` and `tests/release_resource_governor.rs`; TUI `src/main.rs`.
- Owning contracts: harness, ACP, TUI and foundation `AGENTS.md`; owning RRC PRD status and evidence index.
- Root records the expanded autonomous RRC repair preference. `crates/`, `apps/` and `docs/` parent ownership and child boundaries remain unchanged. No new child boundary/index is required.

## Live closeout

The native production controller resumed the original published epoch and completed at `2026-10-06T14:42:00.683531399Z` (milestone 45). It reobserved current main `a56f0ba76525bf4e7e288eefe2dff731cb120914`, all 11 exact-SHA required CI jobs, the 8/8 retained local gates and 16 published assets. The ledger is `Complete`, liveness `idle`, with no in-flight operation. Release commit, annotated-tag object, asset names and retry budget were compared before/after and remain equal.

Existing [registry PR 539](https://github.com/agentclientprotocol/registry/pull/539) remains OPEN on `agent-vesper/v0.20.51`, now at `5a90637a5c7b0c426bea96c770fb67c9927b5f2f`; verified manifest blob `6147513a84cc4ed1a93bc9a96d8a3658d4c3d4e7` contains 0.24.6 and all five published archive digests. Only that manifest changed; the existing icon remains untouched. No PR was opened, replaced or merged.

[Native release closeout report](release-v0.24.6-closeout.md) retains the full structured receipt. Its original source-worktree location is recorded inside it. [Repair evidence](2026-10-06-rrc-automatic-closeout-repair-evidence.json) binds that receipt, PR head and retained log hashes.

A disposable owner under `/tmp/vesper-closeout-owner` invoked the production `release_command_for_workspace_with_factory` resume route, retained its process until the native completion receipt, and compared immutable fields. It registers zero providers, preventing inference or a provider-backed repair; permission remains the authorized Code/Bypass native route with the configured command firewall. It performs no release mutation itself. Commands: `cargo build --offline --manifest-path /tmp/vesper-closeout-owner/Cargo.toml`, then its debug executable, log `/tmp/vesper-closeout-live.log`. This was the authorized live closeout, separate from foundation verification. Its first compile exposed an Option/Result conversion in disposable owner code; corrected before execution.

The published [v0.24.6 release](https://github.com/99percentgrip/agent-vesper/releases/tag/v0.24.6), installation and repository version remain unchanged. Native report/evidence links were written locally in the original source worktree and copied into this repair change; no documentation-only push occurred.

## Deviations, unresolved items and readiness effect

- Initial added test fixture failed compilation due to incomplete mock trait methods and a wrong fake-session accessor; both were corrected before the passing receipt. This was fixture code, not a source regression or a successful test run.
- Ignored tests remain ignored; no platform-wide or live-provider claim follows from Linux fixtures.
- Hosted canonical/MSRV/five-target/web-driver checks for this repair commit are not run or publication-certified. Existing v0.24.6 hosted receipts belong to its original commit.
- Upstream registry PR merge remains maintainer-owned.
- This repair and its documentation are one local change. This closeout performed no new version/tag/release or installation. Alex subsequently authorized a broader RRC PRD audit, remaining repairs and a new version/release after verification; those follow-up changes belong in the next code candidate. No documentation-only push or local installation replacement is authorized.
