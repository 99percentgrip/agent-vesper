# MCP conversation lifecycle repair — execution report

## Objective and status

2026-09-21. Repair the Playwright MCP page-state reset between calls, preserve
conversation isolation and permission gates in both hosts, document the repair,
and produce a local-only commit. Source baseline:
`94ed16de502e98110498010b399b24659b17a63f`.

**Status: source repair verified on Linux; local-only delivery.** This is not
an installed-binary update, cross-platform release certification, or completion
of the [dependency-setup PRD](../dependency-setup-prd.md). The running installed
Vesper has not been replaced. [ADR 0031](../adr/0031-conversation-owned-mcp-stdio.md)
records the lifecycle refinement without rewriting accepted ADR 0013.

## Diagnosis and implementation

The old hosted route constructed a fresh stdio MCP subprocess for every call.
Navigation therefore did not establish the browser context of a later snapshot.
The repair retains an initialized process in an explicit conversation-owned
`McpSession`, not a process-global configuration cache.

- Model-facing discovery, `browser_ui`, `mcp_call` and deferred `mcp__` calls
  share the same owner and monotonically increasing request IDs.
- Up to 16 connections per owner; bounded channels and byte limits; maximum
  60-second operation deadline with cancellation polling. Concurrent requests
  refuse as busy. Cleanup adds a cooperative EOF grace before direct-child kill
  and reap; the operation deadline is not a hard OS process-reaping deadline.
- Transport failure quarantines the connection. No automatic reconnect/replay of
  possibly effective actions. Explicit close/reset releases quarantine; changed
  configurations refuse reuse. Closing an unused browser starts no subprocess.
- MCP `isError` becomes a failed hosted result rather than a success receipt.
  Unscoped discovery retains healthy results and reports each unavailable server;
  requested-server discovery still fails truthfully.
- ACP owners are keyed by session ID and retained across turn registry rebuilds;
  clear-history removes that owner. TUI direct/VRO/ReAct registries preserve the
  gateway and one owner, reset before loading a different transcript. Worker
  services receive fresh owners. Permission-restricted registries remove gateways.
- HTTP remains on the existing per-call bounded transport. One-shot `McpClient`
  and existing host `/mcp tools` inspection remain compatibility paths.

## Files and DOX pass

Production and test changes:

- `crates/vesper-mcp/src/{lib,mcp,session,session_tests,playwright_live_tests}.rs`.
- `crates/vesper-harness/src/{lib,mcp_session_tests}.rs`.
- `apps/agent-vesper-acp/src/lib.rs` and the MCP-only hunks of
  `apps/agent-vesper-tui/src/main.rs`.

Updated nearest MCP, harness, ACP and TUI `AGENTS.md` contracts, ADR ownership,
foundation report ownership, evidence index and owning dependency-setup PRD.
Root, `crates/AGENTS.md`, `apps/AGENTS.md` and `docs/AGENTS.md` are intentionally
unchanged by this repair: no top-level boundary or child index changed.
Unrelated pre-existing voice code, manifests, lockfile and documentation are
excluded from the commit. Mixed files were staged by repair-only hunks or
baseline-plus-repair doc transformations, without discarding working content.

## Methods and exact verification receipts

The staged source was exported with `git checkout-index --all` to
`/tmp/vesper-mcp-repair-check` and tested independently of unrelated uncommitted
voice work. Temporary logs below are execution aids; the decisive receipts are
retained verbatim here. No live provider endpoint, personal Chrome profile,
installer or installed Vesper replacement was used in verification.

### Component and host integration

`cargo test -p vesper-mcp -p vesper-harness -p agent-vesper-acp -p agent-vesper-tui --lib --bins`
returned exit 0 in the isolated tree. In package execution order (ACP library,
ACP binary, TUI library, TUI binary, harness, MCP):

```text
test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 241 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 155 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
test result: ok. 124 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1.04s
test result: ok. 24 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.51s
```

The MCP fixtures cover navigation/snapshot/interaction, owner isolation,
close/reset, configuration change, direct-child reaping, capacity, cancellation,
timeout, disconnect, oversize responses, RPC failures and quarantine/no replay.
The harness fixture exercises the real subprocess through discovery, preset,
explicit call and prefix routes. ACP/TUI tests verify actual composition ownership
and gateway registration, not a real editor/terminal user journey.

### Real isolated Playwright acceptance

Node `24.21.0` and installed Playwright MCP `0.0.82` were supplied explicitly:

```sh
VESPER_TEST_NODE=/home/Alex/.local/share/agent-vesper/mcp-runtime/node-v24.21.0-linux-x64/bin/node \
VESPER_TEST_PLAYWRIGHT_CLI=/home/Alex/.npm/_npx/9833c18b2d85bc59/node_modules/@playwright/mcp/cli.js \
cargo test -p vesper-mcp real_playwright_navigation_snapshot_click_isolation_close -- --ignored --nocapture
```

The test serves a local loopback HTML fixture, launches headless isolated Chrome,
uses the advertised `browser_click` schema (`target`, not `ref`), checks a later
snapshot retains navigation, clicks a real button and checks changed text,
verifies a second owner lacks that page, then closes both browsers. Exit 0:

```text
PASS: real Playwright navigation -> snapshot -> click; independent owner blank; both browsers closed
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 24 filtered out; finished in 2.15s
```

This explicitly run acceptance is ignored in ordinary foundation tests so those
tests do not launch a real browser or require local Node installations.

### Workspace gates

All commands below passed in the isolated repair tree:

- `cargo test --workspace --all-features`: exit 0; summed Cargo result lines,
  including doctests, **2481 passed, 0 failed, 38 ignored** (166 result summaries).
  Ignored tests remain unexecuted by this command; the real browser gate above
  is separately executed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `cargo xtask architecture`: exit 0, `architecture boundaries validated for 28 packages`.
- `cargo xtask verify`: exit 0. Final receipt:

```text
Acceptance regression gate: 23 exact cases passed in 51990 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

- `cargo build --release -p vesper-mcp`: exit 0:

```text
    Finished `release` profile [optimized] target(s) in 9.41s
```

This confirms release compilation, not a release deployment or a new plugin
security claim. The existing unsigned-loader compile-time erasure is retained.

### Failed attempts, deviations and audit corrections

1. The final audit added `closing_unused_session_does_not_spawn_a_server` before
   fixing unused-close behavior. Its first execution failed, exit 101:

   ```text
   test session_tests::closing_unused_session_does_not_spawn_a_server ... FAILED
   assertion failed: McpSession::default().call_tool(&config, "browser_close", json!({})).is_ok()
   test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 24 filtered out; finished in 0.00s
   ```

   The fix returns an explicit no-retained-session result without starting a
   nonexistent executable; the final 24-test MCP suite passes. Thread-start
   failure handling also kills/reaps an already-spawned direct child.

2. The first broad workspace test and `xtask verify` runs inherited
   `CARGO_TARGET_DIR` into nested fixture builds, incorrectly sharing their Cargo
   artifact directory. Two verifier tests failed (`cargo_check_fails_on_broken_crate`
   and `cargo_test_passes_on_valid_crate`), with 422 passed/2 failed in that suite.
   Rerunning without that inherited variable passed. Top-level target reuse used
   Cargo's command-line build setting and then a temporary-tree target symlink;
   fixture builds kept independent target directories. No source test was weakened.

3. An independent delegated review was attempted but did not execute. Exact safe
   failure: `Selected OpenAI model is not in the current account model list; reopen Settings and choose an available model`.
   This report claims a manual source audit plus executable checks, **not** a
   completed independent review. No credential/model setting was changed to bypass
   that failure.

4. Audit scope clarification: bounded operation I/O does not prove bounded
   arbitrary descendant-process teardown. Direct-child reaping is tested on
   Linux; MCP close acknowledgment is not proof that every browser descendant
   exited. HTTP cancellation after dispatch is unchanged, not upgraded here.

## Final audit and evidence boundaries

Re-derived invariants: ownership follows conversation identity, every production
model-facing route reuses that owner, independent owners differ, registry rebuilds
do not recreate it, failures cannot silently replay actions, and gateway additions
do not survive restricted-registry filtering. Source inspection and the named
fixtures support those claims. Unscoped partial discovery received source review
but no dedicated all-presets offline regression in this patch.

Remaining acceptance limits:

- Windows/macOS, MSRV and five-target release CI have not been run here.
- No installed-native-host end-to-end user acceptance or upgrade was performed.
- ACP owner entries otherwise remain until engine shutdown; this repair does not
  introduce idle-session eviction or a process-wide connection budget.
- Arbitrary descendant cleanup, noncooperative inherited-pipe worker shutdown and
  reversal of external effects after cancellation are not certified.
- Independent delegated review remains unavailable as recorded above.
- Existing native dependency-setup and platform PRD gates remain open.

## Readiness effect

The observed cross-call Playwright state reset is repaired in source and passes
real local browser continuity/isolation acceptance. Both host compositions use
the shared route, with complete offline workspace verification passing on the
repair-only tree. This is suitable for the requested local source commit, not an
authorization to push, tag, release, install or replace Alex's local application.
