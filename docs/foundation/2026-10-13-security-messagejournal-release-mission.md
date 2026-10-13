# Security and MessageJournal release mission

## Objective

Complete Alex's authorized PR #42 security remediation, exact-source CI,
integration and native RRC patch release. Preserve the dirty primary checkout and
unrelated worktrees. Installation, voice changes and sidebar redesign are excluded.

## Source and dependency evidence

Starting UX commit: `88f9a36c14c12e57db860e7921b95502f9d1b9d2`.
Security commit/final PR head: `735d4bc7e4d87ea28d5421130d946bd2fb56c653`.
[PR #42](https://github.com/99percentgrip/agent-vesper/pull/42) merged as
`9c37704c52b6dd22ca689d543b56f2856e89f51b`. Their tree diff is empty.
Clean continuation worktree: `/tmp/vesper-security-release-20261013`.

`reqwest 0.13.4` → `quinn 0.11.11` → `quinn-proto 0.11.16` now resolves
`quinn-proto 0.11.18`, whose published manifest declares Rust 1.85 (workspace
MSRV: 1.88). Only its package version/checksum changed; `tempfile` also selected
already-present `getrandom 0.3.4` instead of `0.4.3`. No other package version
changed and no advisory was ignored.

Inspected local RustSec patched thresholds:

| Advisory | Minimum patched | Candidate |
|---|---|---|
| RUSTSEC-2026-0338 | 0.11.18 | 0.11.18 |
| RUSTSEC-2026-0339 | 0.11.17 | 0.11.18 |
| RUSTSEC-2026-0340 | 0.11.18 | 0.11.18 |
| RUSTSEC-2026-0341 | 0.11.17 | 0.11.18 |
| RUSTSEC-2026-0342 | 0.11.17 | 0.11.18 |

## Local verification

Original retained `/tmp/quinn-xtask-verify.log`: `VERIFY:0`, 165 exact acceptance
cases passed, and both `terminal_show_more_restore_copy_and_queue_are_distinct`
and `terminal_message_queue_and_paste_lifecycle` passed. See the
[security receipt](2026-10-13-quinn-proto-security-remediation.md) and
[live UX receipt](2026-10-13-messagejournal-final-ux-acceptance.md).

Independent continuation checks:

- `cargo audit --no-fetch --file <integration-worktree>/Cargo.lock --json`:
  exit 0, 1,309 local advisory records, 554 dependencies, zero vulnerabilities,
  `ignore: []`. No fresh remote database fetch is claimed.
- `cargo deny --all-features --frozen check`: exit 0; retained log
  `/tmp/vesper-security-takeover-deny.log`. The initial sandbox advisory-lock
  denial was resolved by an approved host rerun. An earlier unsupported
  `--disable-fetch` invocation was corrected to `--frozen`.

## Exact GitHub evidence

Final PR head completed successfully:

| Workflow | Run |
|---|---|
| Canonical, supply-chain, Windows installer | [38390007021](https://github.com/99percentgrip/agent-vesper/actions/runs/38390007021) |
| Five-target foundation | [38390007032](https://github.com/99percentgrip/agent-vesper/actions/runs/38390007032) |
| MSRV | [38390007108](https://github.com/99percentgrip/agent-vesper/actions/runs/38390007108) |
| Web driver | [38390007056](https://github.com/99percentgrip/agent-vesper/actions/runs/38390007056) |

Merged main has green canonical
[38398972058](https://github.com/99percentgrip/agent-vesper/actions/runs/38398972058),
MSRV [38398972096](https://github.com/99percentgrip/agent-vesper/actions/runs/38398972096)
and web-driver [38398972094](https://github.com/99percentgrip/agent-vesper/actions/runs/38398972094).
Foundation [38398972113](https://github.com/99percentgrip/agent-vesper/actions/runs/38398972113)
completed successfully on all five targets. The complete matrix settled before
release admission; no failed, skipped or missing target was counted as a pass.

## RRC and remaining work

Old primary epoch `20260929T235130Z-0b5630d27196` is cancelled and will not
be reused. Repository-wide epoch `20261012T142611.269580621Z-5a3ca226734a`
is Complete/Idle for published [v0.24.13](https://github.com/99percentgrip/agent-vesper/releases/tag/v0.24.13),
with no in-flight operation. Next patch: v0.24.14, subject to final remote and
native controller checks. This continuation has not admitted a new RRC release.

The previous agent's saved turn ended with `xAI service request failed`.
The separate native TUI confirms the repository-wide RRC is Complete/Idle.
No active release operation was observed before this continuation's admission.

Status: IN PROGRESS. Remaining: new native RRC admission,
controller-owned local/exact-SHA/publication gates,
verified assets/platform coverage and final delivery. No release success claim.

## Methods, files, deviations and readiness

Inspected worktree identity/cleanliness, PR/diff, lockfile delta, crate manifest,
RustSec records, retained logs, workflows/jobs and RRC ledgers. Commands:
`git worktree list --porcelain`, `git status --porcelain`, `git diff`, `git show`,
`gh run list`, `gh api .../releases/latest`, audit and deny above. GitHub plugin
covers PR/jobs; `gh` covers main-push workflows, which the plugin commit wrapper
does not return. No live provider call or installer was used for verification.

Changed files: this report, evidence-index/PRD links and foundation DOX ownership;
release provenance binds this verified merged security/UX source. Document content,
links, JSON and whitespace checks apply. Parent contracts/child indexes remain
unchanged because ownership boundaries did not change. Platform readiness comes
from exact CI, not the Linux-only process observations or local fixtures.
Readiness effect: security remediation, PR integration and complete merged-main
CI verified; release pending. The candidate's provenance completion time refers
to the verified implementation, not successful publication.
