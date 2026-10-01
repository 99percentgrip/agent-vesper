# Corrected v0.24.4 release preparation

**Date:** 2026-10-01

## Objective

Correct the rejected manual `3.3.1` prerelease mutation while preserving every integrated RRC, Host Resource Governor, OpenAI Responses, origin/main CI, regression and relevant documentation change. Reconstruct one clean source at exactly `0.24.4`, run the complete local gates under the governor, and build one fresh Linux x86-64 TUI that leaves the authorized `0.24.4 -> 0.24.5` transition entirely to RRC after Alex's natural-language release authorization.

## Constraints held

- No release, push, tag, GitHub Release, asset publication, Registry submission, installer or local-install replacement was performed.
- No VRO-19 file or behavior was changed.
- No live provider call was made.
- The dirty primary checkout was not cleaned, reset, stashed or used as the integration workspace; its `git status --short` inventory hash remained `b75adabf1ea69db19907ae1a3e447200f6b31181f9cca973e9c114aec75ec277` across reconstruction.
- Every Cargo metadata, verification, acceptance and release-build command ran through the integrated Host Resource Governor with `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, a managed target directory and critical-pressure stop authority.

## Corrected source identity

```text
BASE_SOURCE_SHA=20584fc5f17bb1d3f1c5b51a86bfded343cb7355
BUILD_SOURCE_SHA=3ae4be17a766b690e02af47f687083bee32c958b
SOURCE_WORKTREE=/home/Alex/Projects/agent-vesper/.worktrees/release-prep-v0.24.4-corrected-2026-10-01
REMOVED_3_3_1_VERSION_COMMIT=1ea840fb0afccbc369e2a5059cc582f94cf78a02
```

`20584fc5` is the clean integrated tree immediately before the invalid version commit. The correction replayed only the two validated post-version source/test repairs:

```text
ef10d57c5a8841af08248f70a768e9cd4b1107c3  test(openai): expect safe ACP rejection text
3ae4be17a766b690e02af47f687083bee32c958b  fix(release): compile terminal probe helper only in debug builds
```

The rejected `1ea840fb` commit is not an ancestor of the corrected source. Its two descendants were not merged; their one-file patches were replayed onto the pre-version tree. The integrated ancestry retains the RRC resource-governor snapshot, autonomous lifecycle snapshot, source-resolution objective, OpenAI Responses repair, origin/main red-CI corrections and reconciled documentation.

## Version-state proof

A locked Cargo metadata inventory and direct TOML/JSON audit produced:

```text
ROOT_WORKSPACE_VERSION=0.24.4
WORKSPACE_OWNED_PACKAGES=31
WORKSPACE_OWNED_PACKAGE_VERSIONS=0.24.4
INTERNAL_EXACT_PATH_PINS=118
INTERNAL_EXACT_PATH_PIN_VALUE==0.24.4
LOCAL_CARGO_LOCK_PACKAGES=31
LOCAL_CARGO_LOCK_VERSION=0.24.4
REGISTRY_VERSION=0.24.4
REGISTRY_RELEASE_URLS=5
REGISTRY_RELEASE_URL_VERSION=v0.24.4
RELEASE_OWNED_3_3_1_OCCURRENCES=0
WHOLE_TRACKED_TREE_3_3_1_LINES=0
VERSION_CONTRACT=PASS
```

The double `==` in the display line is the field delimiter followed by the exact Cargo requirement `=0.24.4`; all 118 internal path dependencies use that exact requirement. The audit restricted the release-owned mutation check to every path changed by `1ea840fb` and separately searched the complete tracked tree. Neither contained `3.3.1`.

## Methods and commands

Representative commands:

```sh
git worktree add --detach <corrected-worktree> 20584fc5
git cherry-pick 5304b9935c595ddb9015f0a0742e271ae1e4a9b3
git cherry-pick 94c2f6e29969f44c8eb0e50221c83ae61aecafd2
cargo metadata --locked --format-version 1
cargo xtask verify
cargo xtask acceptance
cargo build --locked --release --package agent-vesper-tui \
  --features docker,swarm,bridge,voice-kokoro,voice-flm
```

The Cargo commands were children of the governor runner. The runner SHA-256 is `498114dd8107dbf3b38f94cfbd06240b31dada5b327aaa54a4c923fb6cbd5c10`; its embedded `host_resources.rs` source and the corrected source both hash to `0765d39a5b82ea245312be673fd5e8457f19b0e1c6ee592aeb1c965e16c78059`.

## Exact verification evidence

### Complete verification

`cargo xtask verify` exited zero. Its final acceptance phase reported:

```text
Acceptance regression gate: 44 exact cases passed in 132323 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The command also completed format, strict all-target/all-feature Clippy, workspace tests, doctests, architecture, fixtures, contracts, provider, runtime, ACP, sessions and naming checks.

### Explicit acceptance rerun

The separately required `cargo xtask acceptance` exited zero:

```text
Acceptance regression gate: 44 exact cases passed in 21849 ms. Offline fixture model cost: zero; live-model effectiveness is not measured.
```

The 44-case set included natural-language RRC admission/resume, stale-epoch replacement, unrelated/irreversible clarification, registered task ownership, publication gating, host resource constraints, progress projection, ACP governor inheritance and real-PTY child-output ownership.

### Resource Governor

All pressure observations remained `normal`; no command was stopped for critical pressure.

| Command | Minimum available RAM | Peak owned-tree RSS | Maximum swap used | Minimum disk headroom | Exit |
|---|---:|---:|---:|---:|---:|
| `cargo xtask verify` | 19,501,694,976 B | 406,892,544 B | 1,431,924,736 B | 303,220,596,736 B | 0 |
| `cargo xtask acceptance` | 20,482,711,552 B | 122,044,416 B | 1,431,666,688 B | 302,880,346,112 B | 0 |
| release TUI build | 19,440,541,696 B | 119,721,984 B | 1,431,666,688 B | 302,312,124,416 B | 0 |

Each receipt records admission, process-tree observations, inherited Cargo environment, pressure transitions, exit status and whether the owned group was stopped.

## Final candidate

```text
TUI_PATH=/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-corrected-integrated-linux-x86_64-20261001/agent-vesper-tui
TUI_SHA256=6d11bce97a438aded2c6c7cd3ba634a4f92bda26d67318e2723a9b8f96ff75d3
TUI_VERSION=0.24.4
SOURCE_SHA=3ae4be17a766b690e02af47f687083bee32c958b
```

The executable is a stripped Linux x86-64 release-profile ELF. Its SHA differs from the rejected `3.3.1` candidate SHA `9aa1ddbd775e92076c80e0ea70bf11808c92c220daf2f7c16c3ebb6d47d79a9e`. `CHECKSUMS.sha256`, a self-excluding all-files manifest, provenance, version proof and governor receipts are stored beside it.

Exact launch command:

```sh
/home/Alex/Projects/agent-vesper-prerelease-candidates/v0.24.4-corrected-integrated-linux-x86_64-20261001/agent-vesper-tui
```

## Files changed by the correction

- Replayed the safe ACP OpenAI rejection expectation into `apps/agent-vesper-acp/tests/openai_rejection.rs` without the invalid version ancestry.
- Replayed the debug-only terminal probe compile guard into `crates/vesper-harness/src/release_executor.rs` without the invalid version ancestry.
- Added this correction report and its evidence-index, RRC PRD and nearest-DOX links.
- No Cargo manifest, `Cargo.lock`, Registry manifest or release URL was changed by the corrected source reconstruction.

## Deviations

1. The earlier `3.3.1` candidate is rejected and superseded. It remains only as historical local evidence; it must not be launched for the live release acceptance.
2. The first version-audit script compared exact pins to bare `0.24.4`; the repository correctly uses `=0.24.4`. The corrected audit required that exact syntax and passed all 118 pins.
3. This report is a documentation-only closeout commit after the exact build source. It does not alter production code or the candidate bytes; `SOURCE_SHA` above remains the actual build input.
4. Linux-local verification does not replace hosted exact-SHA or other-platform release gates. Those remain RRC's responsibility after authorization.

## Unresolved items

- Alex has not yet issued the natural-language release authorization from this corrected binary.
- The `0.24.4 -> 0.24.5` mutation, release candidate commit, push, GitHub matrices, annotated tag, GitHub Release, platform assets/checksums and Registry update remain unexecuted.
- Cross-platform exact-SHA execution remains pending.
- No live provider acceptance was run for this candidate.

## Readiness effect

The corrected Linux prerelease TUI is ready for Alex's user-operated RRC acceptance. It is intentionally version `0.24.4`. The only authorized next release target is `v0.24.5`, and only Alex's exact natural-language request—`Release all completed work as the next patch.`—authorizes RRC to begin that real release. This report does not authorize or perform release actions.
