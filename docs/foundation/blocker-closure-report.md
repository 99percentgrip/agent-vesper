# Blocker closure and Stage 1 readiness

Status: COMPLETE

## Objective

Consolidate the blocker-resolution evidence, distinguish local proof from
unexecuted platform validation, and decide whether creating the production Cargo
workspace is authorized and safe.

## Blocker closure matrix

| Blocker | Initial state | Work performed | Evidence | Final state | Blocks production foundation? |
| --- | --- | --- | --- | --- | --- |
| Source test stall | Open | Reproduced focused, ordered group, and full suite under five isolated state roots with bounded timeouts and process inspection | `source-test-stall-investigation.md`; 879/879 pass | RESOLVED | No |
| Product decisions | Open | Eight decision records separated parity requirements from recommendations and future changes | `decision-register.md`; ADR 0001–0008 | PRODUCT APPROVAL REQUIRED | Yes |
| Fixture schema | Missing | Versioned language-neutral manifest/result schemas and normalization rules | `fixture-charter.md`; `fixtures/schema/*` | RESOLVED | No |
| Python oracle | Missing | Isolated source runner, commit verification, canonical capture, schema/canary/index checks, process cleanup | `python-oracle-report.md`; 65 scenarios; 132 hashes | RESOLVED | No |
| ACP Rust SDK validation | Missing | Pinned official crate 2.0.0, wire-v1 transcript/dispatch/shutdown tests | `acp-rust-sdk-spike.md`; 7/7 pass | RESOLVED LOCALLY — CI VALIDATION PENDING | No |
| SSE transport proof | Missing | Pinned reqwest 0.13.4, local raw server, bounded parser and cancellation/partial-output tests | `rust-sse-transport-spike.md`; 10/10 pass | RESOLVED LOCALLY — CI VALIDATION PENDING | No |
| Process cleanup proof | Missing | Real Unix process groups, descendants, drop/cancel/timeout cleanup, drain and FD tests | `process-sandbox-spike.md`; 9/9 pass | RESOLVED LOCALLY — CI VALIDATION PENDING | No |
| Sandbox platform proof | Missing | Bubblewrap locally; executable Seatbelt and Job Object probes plus five-target workflow | Linux 3/3 pass; macOS/Windows unexecuted | RESOLVED LOCALLY — CI VALIDATION PENDING | No for workspace; yes for cross-platform release |
| SQLite FTS5 packaging | Missing | Bundled and system builds, FTS5/WAL/ranking/redaction/rebuild/fail-soft tests | `sqlite-fts5-spike.md`; 6/6 each configuration | RESOLVED LOCALLY — CI VALIDATION PENDING | No |
| Repository initialization | Pending | Preserved all content; initialized `main`; no commit/remote; added only foundation ignore rules | Git status and `decision-register.md` | RESOLVED | No |

## Technical conclusions

### Source baseline

The frozen Python suite is green on the current Linux host: 879 collected,
879 passed, zero failures/skips, 89.41 seconds, normal exit, and no matching
descendant processes. The previously reported focused stall did not reproduce
alone, after related tests, or in the complete suite. It is classified as an
environment/executor observation, not a demonstrated source deadlock.

One separate source defect was reproduced during fixture work:
`glm_acp/tools.py::_run_command` handles timeout cleanup but not coroutine
cancellation. The source oracle saw two descendants before oracle cleanup. This
does not block fixture capture; it becomes an explicit Rust security regression
test.

### Fixture and oracle foundation

The canonical corpus contains:

| Category | Scenarios |
| --- | ---: |
| ACP | 12 |
| Provider/GLM | 21 |
| Sessions/v1 | 7 |
| Tools | 10 |
| Policy | 6 |
| Security | 5 |
| Process | 4 |
| **Total** | **65** |

All 65 manifests/results validate. The hash index verifies 132 payloads (two
schemas plus 65 manifest/result pairs). Two independent deterministic recaptures
produced the same complete index hash:
`27e58c39fe95882961bf877b132b4ecbc6209850c57cd801fc2219e345632f86`.
Secret-canary checks passed.

Some broad tool/security scenarios intentionally retain focused Python-test
evidence rather than pretending to be full wire transcripts. The report labels
them as foundation canaries; richer variants are a Stage 1 test-expansion task,
not an absent critical-path contract.

### Rust technical spikes

- ACP: suitable behind a compatibility wrapper. The current official crate is
  2.0.0 while the wire version remains v1. Fork is unstable, source
  `PromptResponse.userMessageId` differs from current schema placement, and
  blocking callbacks must be kept off inbound dispatch.
- SSE: suitable behind a harness-owned bounded parser. Exact cancellation,
  timeout, EOF, partial-output, retry, and no-reconnect rules pass locally.
- SQLite: `rusqlite` 0.40.1 with bundled SQLite is the recommended release
  default; system SQLite is a tested opt-in for development/distributions.
- Process/sandbox: a new process group plus terminal-path cleanup is viable on
  Linux. Bubblewrap PID/network/write restrictions work. The Rust production
  design must fix, not reproduce, the source cancellation leak.

## Product approvals still required

The following recommendations are not user approvals:

1. Exact Agent Vesper state-root name/location, import command UX, and whether a
   later explicit writer migration is offered. No silent movement or legacy
   overwrite is already a governing requirement.
2. Any future change from default-on reasoning persistence, including retention
   and provider-opaque reasoning policy. Initial parity remains frozen.
3. Whether TUI parity accepts the recommended behavioral/accessibility contract
   rather than pixel identity, and approval for any command/keybinding removal.
4. Long-term MSRV and release-target support policy. Rust 1.88 is the provisional
   floor forced by the selected ACP SDK, and all five source target families are
   provisionally retained.
5. Eventual deprecation timing for legacy shell-string `run_command`. Initial
   compatibility plus a separate argv-native contract is frozen.

Bypass denial, Plan Mode MCP parity, initial reasoning parity, command-contract
separation, and repository initialization are already approved by existing
requirements.

## Cross-platform proof boundary

Only Linux x86-64 ran locally. The workflow uses current official standard
GitHub runner labels for Linux x86-64/ARM64, macOS Intel/Apple Silicon, and
Windows x86-64, but no workflow has run because this repository has no remote or
commit. Therefore:

- Linux x86-64: locally validated.
- Linux ARM64: CI validation pending.
- macOS Intel: CI validation pending.
- macOS Apple Silicon: CI validation pending.
- Windows x86-64: CI validation pending.

No packaging, Seatbelt, Job Object, ACL, or ARM64 claim has been upgraded from
pending without a real host.

## Repository foundation decision

The target is now a Git repository on `main`, with no commit and no remote.
`.gitignore` excludes disposable build/cache output while retaining canonical
fixtures. A production Cargo workspace was **not** created.

`rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, and a
production `docs/adr/` boundary are deferred until the MSRV/support decision is
approved and Stage 1 begins. The completed Stage 0 decisions remain under
`docs/foundation/adr/`; moving or duplicating them would add structure without an
approved production workspace.

## Files created

Permanent work is confined to:

- `docs/foundation/` reports and eight ADRs;
- `fixtures/` schemas, 65 manifests/results, and hash index;
- `tools/python-oracle/`;
- four disposable packages under `spikes/`;
- `.github/workflows/foundation-spikes.yml` and its DOX contract;
- `.gitignore` and updated DOX indexes.

No production Agent Vesper crate or feature implementation exists.

## Unresolved issues and migration effect

- Product approvals above block final production crate/config contracts.
- Five-target CI must run before cross-platform release claims.
- ACP `userMessageId` placement needs an explicit wrapper/raw-wire test during
  Stage 1, but does not prevent creating the workspace skeleton.
- Windows Job containment and macOS Seatbelt remain real, executable tests rather
  than mocked proof.
- The source cancellation leak must remain a negative parity/security fixture.

Technical evidence is sufficient for a later workspace foundation, but the
explicit Stage 0 product choices are not all approved. No workspace should be
created automatically from this mission.

## Readiness

READY WITH PRODUCT DECISIONS PENDING
