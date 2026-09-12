# Foundation Evidence Index

Status: evidence index; acceptance status is scoped to each section.

## v0.22.2 Settings repair release — 2026-09-12

[Release execution](v0.22.2-release-execution.md) tracks the authorized version
bump, exact-commit gates, publication and existing registry PR update. Local
TUI/plugin installation is reserved for the user's own update test.

## Native Settings and confirmed updates — 2026-09-12

[Execution report](settings-and-update-execution.md) traces
[the repair PRD](../settings-and-update-prd.md): shared themed menus, grouped
save/discard, persisted execution choices, protected automatic PRD enrollment,
and a confirmed installer workflow. Final all-feature workspace: 2,234 passed,
0 failed, 34 ignored; changed-crate MSRV: 548 passed, 0 failed, 4 ignored.
Native terminal and checksum/install fixtures passed. Windows native updater
execution remains unverified; no release or user installation was performed.

## Session push: context paging + productization + hardening planning — 2026-09-12

`push-context-paging-execution.md` records pushing the session's three
completed work units to `origin/main` (`637eb7e..a378bd8`, four commits)
after fresh gates on the exact tree: workspace 2,218/0, acceptance 20/20,
naming-guard clean. Two initially misplaced files were corrected pre-push
(one amend, one follow-up docs commit). Ranker-hardening PR-1..3 remain
PLANNING and are the resumption point.

## Ranker hardening PRD (alias cross-talk) — 2026-09-12

`ranker-hardening-prd-execution.md` records drafting
`docs/ranker-hardening-prd.md` from the alias cross-talk recon: scope fenced
to Option A (raw stemmed chunk pools; `rank_chunks` bypasses the alias loop,
skill tier untouched) + Option D (failing cross-talk fixture anchors the
defect before the fix), incident quantified at 1,560 manufactured overlap
points (`skill_orchestrator.rs:869/:873`), bounded 520-pt prompt-side
residual disclosed as accepted, three PRs (anchor → decoupling → re-eval
with a D3-verdict stop rule). migration-status row PLANNING; naming-guard
clean. No code changed.

## Alias cross-talk swarm recon (ranker hardening) — 2026-09-12

`recon-alias-crosstalk-execution.md` records the read-only recon mission
behind `architecture/recon_alias_crosstalk.md`: exact bleed mechanism
(slug tokens enter only via the prompt pool at skill_orchestrator.rs:421-422
and join chunk pools at :869; the PR-4 incident scored 1,560 manufactured
points, not 520), blast radius proven zero over the shipped floor under
either one-sided alias ablation, four decoupling options ranked with
recommendation A+D, and two latent defect classes empirically pinned
(stem-form alias asymmetry `releases→releas`; dead hyphenated alias rows).
No production code changed. naming-guard clean, 18 frozen hits.

## Completion-reporting workflow productized — 2026-09-12

`completion-reporting-workflow-execution.md` records shipping the session's
reporting/audit conventions as default installed behavior: shared
`COMPLETION_REPORTING_INSTRUCTION` in `vesper-harness` injected by both
hosts at every loop build (ACP interactive+worker, TUI direct+tool-loop)
with byte-for-byte parity tests in both apps; the `work-unit-reporting`
seed skill (library 94→95, bundle registered, mirrored) teaches report
structure, final-audit regression-first discipline, and release gates,
composing with `verify-with-xtask-verify`. Workspace 2,218/0, acceptance
20/20, architecture + naming-guard clean, seed count 95==95. Honest
limitation: report-compliance effectiveness of installed agents is not
yet measured by any eval.

## Advanced context paging — full implementation audit (PR-1..PR-5) — 2026-09-12

`context-paging-full-audit.md` records the end-to-end audit: five findings,
all repaired with fail-then-pass regression proofs — F1 per-skill chunk
budget never decremented (reproduced 36,261 > 24,000), F2 SUMMARY_ONLY
overhead measured against the full text (published 16/16 parity was a
metric artifact; true 13–14 vs 16), F3 explicit requests with invalid
manifests failed silently, F4 manifest defects masking `archived`,
F5 the eval control case silently failing activation while the report
narrative claimed controls always succeed. **ADOPT verdict re-derived and
stands on corrected data.** Workspace 2,217/0 (+4 audit regressions),
acceptance 20/20, architecture + naming-guard clean. PR-4 report carries
an explicit audit-correction note.

## Advanced context paging — PR-5 (docs & authoring; initiative COMPLETE) — 2026-09-12

`context-paging-pr5-execution.md` closes the five-PR initiative: chunk
authoring guidance in `skills/AGENTS.md` + `vesper-skill-authoring` v1.1.0
(seed mirrored to the global home, byte-identical), matching shipped
behavior exactly (32/24,000 caps, ≤3 loaded, summary/key_elements actively
routing post-ADOPT, fail-closed semantics, vocabulary-competition
pitfall). Explicit no-user-surface finding (zero `apps/` changes).
migration-status row → COMPLETE. Floors intact: workspace 2,213/0,
acceptance 20/20, naming-guard clean, seed count 94. Initiative-level
open items (CI matrix, chunked seed exemplar, alias cross-talk remedy)
recorded in §7 of the report.

## Advanced context paging — PR-4 (D3 eval gate; verdict ADOPT) — 2026-09-12

`context-paging-pr4-eval.md` records the D3 decision: strict
three-condition ablation (FLAT_DESCRIPTION / SUMMARY_ONLY /
SUMMARY_KEY_ELEMENTS) over a 2-family × 2-probe corpus in
`crates/vesper-memory/tests/chunk_routing_eval.rs`, entirely offline.
Improvement repeated across BOTH families with distinct marginal-value
carriers; measured overhead 13–14 (SUMMARY_ONLY) / 16 (SUMMARY_KEY_ELEMENTS)
semantic tokens/skill (audit-corrected); zero regression on
controls. Verdict ADOPT; `CHUNK_METADATA_ROUTING_ENABLED = true` flipped
in the same change. Material finding: production `SEMANTIC_ALIASES`
cross-talk (`deploy → release`) can outvote honest key-elements matches —
corpus hardened, production remedy deferred with its own evidence bar.
Workspace 2,213/0, acceptance 20/20, architecture + naming-guard clean.

## Advanced context paging — PR-3 (composition & injection) — 2026-09-12

`context-paging-pr3-execution.md` owns the PR-3 execution record: chunk
payloads moved onto `LoadedSkill`, named
`<agent-vesper-skill-chunk>` emission in `context()` after the primary
slice, transient host-append/restore (AC-3), and direct/VRO/ReAct
seam-parity proofs in `crates/vesper-harness/tests/context_paging_composition.rs`
(6 tests, real AgentLoop + FakeProviderSession capture). Material
architecture finding: `vesper-agent` is deliberately skill-unaware and the
architecture gate caught a `vesper-agent → vesper-memory` dev-dep edge —
tests correctly relocated to the harness composition boundary. Workspace
2,208/0, acceptance 20/20, architecture 27 packages, naming-guard clean.

## Advanced context paging — PR-2 (two-level routing) — 2026-09-12

`context-paging-pr2-execution.md` owns the PR-2 execution record:
bounded second-pass chunk routing (`MAX_CHUNKS_PER_SELECTION = 3`,
description-only feed while `CHUNK_METADATA_ROUTING_ENABLED = false`),
chunks counted against per-skill/total budgets with fail-closed skip
(never truncated), isolated skills excluded, and all four AC-2 proof
categories in `tests/skill_routing.rs` — including the distinguishing
budget-boundary case (chunk under the byte cap but over the per-skill
allowance → rejected *by budget*) and the flag-off neutrality proof
(summary-only overlap loads nothing). Workspace 2,202/0, acceptance
20/20, architecture + naming-guard clean. Deviations (compile-time flag,
envelope emission deferred to PR-3) and open items recorded.

## Advanced context paging — PR-1 (storage & manifest) — 2026-09-12

`context-paging-pr1-execution.md` owns the PR-1 execution record: chunk
storage enumeration, manifest parse, fail-closed caps (32 chunks / 24,000
bytes, field caps), G5 zero-regression proofs, and the exact gate receipts
(workspace 2,197/0 vs the 2,185+ floor, `cargo xtask acceptance` 20/20,
architecture 27 packages, naming-guard clean). Includes a live sabotage-run
note: a fixture path mistake made three tests fail with the exact
fail-closed rejection, proving the manifest-vs-disk gate bites. Deviations
(chunk-dir layout matching `references/` convention, added field caps) and
open items (`MAX_CHUNKS_PER_SELECTION` for PR-2, D2 routing-neutrality,
CI pending) are recorded. Scope: PRD PR-1/AC-1 only; routing, composition,
and eval remain future PRs.

## Completion assurance research — 2026-09-11

`completion-assurance-proposal.md` records source inspection at `8083f9f`,
primary-source open-source research, and a proposed Rust completion gate.
The inspected loop accepts model-updated plan completion without a requirement
evidence verdict; streamed content can precede the terminal decision. The proposal
separates coverage review, observed verification, and permission to claim completion.
Status: research/design only; no runtime implementation, dependency installation,
provider experiment, release, or measured effectiveness claim.

## VRO-15 asynchronous lease and contained native Hive checkpoint

- `vro15-repair-execution.md` owns the current F01–F18 matrix and exact resume
  point. Reservation/dispatch/commit moves all backend lease operations outside
  bookkeeping locks, retains cancelled/hung ownership, and exposes explicit
  cleanup outcomes. The native factory now binds scoped routes across command
  continuations, scale, retirement and replacement.
- Real Podman testing reproduced SELinux mount and stop-grace defects, then passed
  after explicit private worker-root labels and bounded ephemeral stop semantics.
  All four topologies pass real 1+3-worker commands, approvals, synthesis,
  scale/replacement, post-replacement execution and clean shutdown. The same-image
  x86_64/ARM64 CI gate is wired; CI execution remains pending.
- Canonical and Rust 1.88 full workspace: **2,049/0/25**; default: **2,006/0/17**.
  Strict Clippy, formatting, architecture, naming, supply chain and whitespace pass.
  Explicit real pipe-browser, Chrome interview and both bounded ledger scale bodies
  pass. Namespace admission remains blocked by `/proc/self/uid_map` permission
  denial, including an authorized outside-sandbox attempt.
- Full VRO-15 is **not complete**: persisted native Settings, both-host command/
  execution composition, project-input delivery, cognition/compaction, full Lens
  continuation, timestamp/high-dimensional/property work and external gates remain.
  This checkpoint does not authorize activation or a release.

## VRO-15 lease-port unwind containment

- Acquire/release/retry panics close admission without poisoning lease bookkeeping;
  uncertain boundaries remain reserved and queued callers wake closed. Two tests
  cover partial creation, cleanup/retry panic and holder unwind.
- Swarm suite: **269 passed, 0 failed, 3 ignored**. Package strict Clippy, Rust 1.88
  locked check, architecture, naming, workspace formatting and whitespace pass.
  Subsequent all-feature workspace suite: **2,036 passed, 0 failed, 23 ignored**.
  Async composition and blocking/supervisor acceptance remain open; see
  `vro15-repair-execution.md` for exact evidence and limitations.

## VRO-15 shared sandbox panic containment

- The shared command adapter converts backend construction/poll unwind into
  quarantine and refusal; a run panic still reaches explicit teardown. Three new
  tests cover provision panics, phase guards and ordinary refusal behavior.
- All-feature harness suite: **101 passed, 0 failed, 0 ignored**. Package strict
  Clippy, Rust 1.88 locked check, architecture, naming, workspace formatting and
  whitespace checks pass. No full-workspace or supervisor acceptance claim.
- Async lease composition, blocking hangs and native activation remain open;
  exact scope and panic-hook limitations are in `vro15-repair-execution.md`.

## VRO-15 Hive membership reconciliation checkpoint

- `vro15-repair-execution.md` records caller-owned pool scaling/health replacement
  wired to concrete routes, topology, inboxes and assignment loads, with fail-closed
  cancellation/reconciliation behavior and navigator failover policy preserved.
- Five regressions pass; workspace all-feature suite: 2,004 passed, 23 ignored.
  Strict Clippy, MSRV compilation, architecture, naming, format and whitespace pass.
- Native monitoring, supervisor cleanup, Settings/host and browser-to-provider Lens
  integration remain open. This is library lifecycle evidence, not full acceptance.

## VRO-15 retirement and generation-cost checkpoint

- `vro15-repair-execution.md` records four reproduced retirement defects repaired
  and graph/entry structural sharing with preserved immutable reader generations.
- Explicit 10k/16D release workload improved from 27.667 s to 1.618 s locally;
  lossless snapshot/continued insertion and 1k bounded retention checks pass.
  These measurements do not certify million-entry memory or native-host latency.
- Workspace: 1,999 passed, 23 ignored; both newly ignored scale measurements were
  separately executed successfully. Strict Clippy, MSRV, architecture, naming,
  formatting and whitespace checks pass. Full swarm/host acceptance remains open.

## VRO-15 interview adapter-boundary checkpoint

- `vro15-repair-execution.md` records matching GLM and native OpenAI serialization
  fixtures; OpenAI covers both auth modes, decoded call identity and next-request
  notes/answers. No production adapter behavior changed. Browser/host end-to-end
  delivery and original HTTP 400 diagnosis remain open.
- Latest all-feature workspace suite: 1,994 passed, 21 ignored. OpenAI: 37 passed;
  GLM provider verification: 59 passed. Workspace strict Clippy, MSRV compilation,
  architecture, naming, formatting and whitespace checks pass locally.

## VRO-15 automatic retention checkpoint

- `vro15-repair-execution.md` records persisted per-scope admission caps, atomic
  record/transfer reservation and version-2 whole-ledger policy validation.
- Four new regressions pass. Latest local verification: 1,992 all-feature workspace
  tests passed, 21 ignored; 246 swarm tests passed, 1 ignored. Workspace Clippy,
  Rust 1.88 compilation, formatting, architecture, naming and whitespace checks pass.
- Scale, lifecycle/cleanup, host composition and Lens/provider-wire acceptance
  remain open; this checkpoint is not full repair or cross-platform acceptance.

## VRO-15 ledger retrieval and retention checkpoint

- `vro15-repair-execution.md` records eight new tests for provenance/category/range
  filters, transactional selective transfer, explicit per-scope retention, semantic
  threshold and configured over-fetch. Immutable reader generations are preserved.
- Latest local verification: 1,988 workspace all-feature tests passed, 21 ignored;
  242 swarm tests passed, 1 ignored. Workspace Clippy, Rust 1.88 locked compilation,
  formatting, architecture, naming and whitespace checks pass.
- Automatic retention/scale, lifecycle/cleanup, Settings/host activation and Lens
  provider-wire acceptance remain open. This is not full swarm or release acceptance.

## VRO-15 native OpenAI rejection diagnostics

- `vro15-repair-execution.md` records bounded, allowlisted rejection diagnostics:
  36 adapter tests, 4 existing native ACP tests, 1 new both-mode ACP rejection test,
  39 Lens tests and 2 TUI native OpenAI wiring tests pass locally.
- Real ACP protocol error data preserves `ContextLimit` without raw provider prose.
  Original HTTP 400 cause, Lens provider-wire delivery and full swarm acceptance
  remain unresolved. Focused checks are not new full-workspace/platform evidence.

## VRO-15 local verification checkpoint

- All-feature workspace recheck: 1,947 passed, 21 ignored; Clippy, Rust 1.88
  compilation, formatting, architecture, naming and whitespace checks pass.
- The prior run had an intermittent web-driver detection fixture failure; focused
  and full reruns passed. See `vro15-repair-execution.md` for the exact limitation.
- This is local partial-repair evidence, not F01–F18 or cross-platform acceptance.

## VRO-15 repair milestone: native AgentLoop adapter

- `vro15-repair-execution.md` records replacement of the stream-only adapter with
  the existing native AgentLoop, real tool transactions, executable role filtering,
  inherited permission/configuration ports and retained interrupted history.
- Six default-off adapter tests pass. Both host DOX documents retain activation
  as open work and remove the unsupported ACP progress exclusion.
- Caller-time bus expiry and pool capacity/timing ceilings have focused acceptance;
  independent worker lifecycle, overlap and full cross-host acceptance remain open.

## VRO-15 repair milestone: lifecycle and execution boundaries

- `vro15-repair-execution.md` records partition failover persistence, actual pool
  cancellation, bounded snapshot serialization, strict structured decomposition,
  scored class routing, retained interrupted goals and grounded synthesis.
- Regression tests cover each changed contract; real independent-worker overlap,
  native host composition and provider-wire feedback remain open acceptance work.
- Cargo metadata architecture checks now enforce optional/default-off swarm edges,
  with unconditional, transitive-default and renamed bypass tests.

## VRO-15 repair milestone: centralized leader wiring

- `vro15-repair-execution.md` records failed-before/passed-after hub-vacancy
  evidence and manual/automatic failover regression coverage.
- Swarm and all-feature workspace tests, crate all-target Clippy, architecture,
  naming guard and whitespace checks pass locally. Partition lifecycle and full
  F01–F18 acceptance remain open; this is not a release-readiness claim.

## VRO-15 repair milestone: assignment scoring

- `vro15-repair-execution.md` records partial repair evidence, not full acceptance.
- Whole-base health scaling and hard candidate eligibility corrected in
  `crates/vesper-swarm/src/hive/assignment.rs`; four desired-behavior regression
  tests added. Hive dispatch integration remains open.
- Local `cargo test -p vesper-swarm`: 167 pass, one ignored doctest.
  Crate all-target clippy (`-D warnings`), architecture, naming guard and diff
  whitespace checks pass. Workspace/MSRV/platform acceptance remains pending.
- VesperLens provider coupling remains an unconfirmed hypothesis.

## VRO-15 repair milestone: cancellation, pool, leases and bus

- See `vro15-repair-execution.md` for scoped acceptance and remaining gaps.
- Timeout signal propagation/caller-drop checks pass. Nine new integration
  regressions across pool, leases and bus failed before fixes and pass afterward.
- Local swarm suite: 176 pass, one ignored doctest; crate all-target Clippy,
  all-feature workspace compilation, architecture, naming guard and whitespace
  checks pass. This is not full workspace test/MSRV/platform or host acceptance.

## VRO-15 repair milestone: HNSW loader hardening

- `vro15-repair-execution.md` records input-budget, identity/edge/layer, finite
  vector and caller-policy validation. Four new loader regressions, 13 existing
  HNSW integrations and crate Clippy pass. RNG/config/raw-vector persistence,
  math/pruning and full snapshot acceptance remain open.

## VRO-15 topology and snapshot publication milestone

- See `vro15-repair-execution.md`: topology vacancy/successor/refusal fixes,
  HNSW v2 raw/RNG/config persistence and math repair, immutable ledger generations,
  transactional transfer, and whole-ledger snapshot format.
- Local 190 swarm tests pass, one doctest ignored; crate Clippy, Rust 1.88 locked
  crate check, all-feature workspace compilation, architecture and naming guard
  pass. Pinned `cargo-deny 0.20.2` in a temporary tool root passes advisory,
  ban, license and source gates with all features.
- Full F01–F18, partition lifecycle, resource/scale, hosts, and VesperLens
  acceptance remain open. This milestone is not full completion.

## VesperLens interview verdict clarification

- `vro15-repair-execution.md`: separate typed planning-answer action, preserved
  notes/choices through JSON and authenticated loopback delivery; 54 Lens tests
  and agent Clippy pass. Both real-Chrome E2E scripts pass using temporary,
  verified Node tooling and Playwright. Workspace: 1,922 tests pass (21 ignored),
  full Clippy, Rust 1.88 all-feature compilation, formatting, architecture and
  naming checks pass. OpenAI-specific loss is not reproduced; provider-wire and
  ACP browser integration remain open acceptance gaps.

## Bus resource bounds milestone

- `vro15-repair-execution.md`: payload/aggregate bytes, subscriber/identity,
  TTL/count and ACK-debt ceilings; four new bounded-resource regressions and
  28 existing bus tests pass. Explicit time composition remains open.

## Mission baseline

| Repository | Fresh evidence | Classification |
|---|---|---|
| Source | `/home/alex/Projects/Native GLM-5.2 Provider`; root matches; `origin=https://github.com/99percentgrip/Native-GLM-ACP.git`; branch `agent/jit-tool-loading`; commit `bf4d4287e2e3320aa3f09015f678e6169d520045`; only `?? docs/codex-tui-roadmap-prompt.md` | Confirmed; immutable |
| Target | `/home/alex/Projects/Agent Vesper`; reconnaissance/DOX files only; not a Git repository at Phase 1 inspection | Confirmed |
| Toolchains | source Python 3.11.15, uv 0.11.14, lock SHA-256 `576101748f90bc6cfd9b098f33e023102ad4931fd346160dfeb02735aea3304e`; local Rust 1.95.0/Cargo 1.95.0 | Confirmed locally |

## Phase ledger

| Phase | Evidence | Documents updated | Status |
|---|---|---|---|
| 1. Repository state | All reconnaissance reports and applicable DOX reread; source identity and target contents reverified | This index | Complete |
| 2. Source test stall | Focused 1/1 and `test_agent.py` 208/208 pass; full suite 879/879 passed in 89.41s with normal exit and no matching descendant | `source-test-stall-investigation.md`, this index | Complete; historical executor stall not reproducible |
| 3. Decisions/ADRs | Eight decisions recorded; Git initialized on `main`; published ACP 2.0.0 establishes Rust 1.88 floor; five target families confirmed | `decision-register.md`, `adr/0001`–`0008`, this index | Complete; product approvals remain explicit |
| 4. Fixture charter/schema | Versioned manifest/result JSON Schemas and normalization/security contract created | `fixture-charter.md`, `fixtures/{README,AGENTS}.md`, `fixtures/schema/*`, this index | Complete; runtime validation follows with oracle |
| 5. Python oracle/corpus | 65 scenarios across 7 categories; 132 schema-validated payloads; canary clean; stable index `27e58c…632f86`; source cancellation leak captured | `python-oracle-report.md`, `tools/python-oracle/*`, `fixtures/*`, this index | Complete locally |
| 6. ACP/SSE Rust spikes | ACP SDK 2.0.0/wire-v1 7/7 pass with wrapper requirements; reqwest 0.13.4 bounded SSE 10/10 pass with exact cancellation/partial-output rules | both spike reports and spike packages, this index | Complete locally |
| 7. SQLite/process/sandbox | rusqlite 0.40.1 bundled 6/6 and local system 6/6 pass; process conformance 9/9 and Linux Bubblewrap 3/3 pass; five-target workflow/scripts prepared | SQLite/process reports, both spike packages, CI workflow, this index | Complete locally; non-Linux/ARM64 CI pending |
| 8. Readiness audit | 65 scenarios and 132 hashes revalidated; all four local Rust spikes rerun; formatting/YAML/shell syntax checked; source identity/status unchanged | `blocker-closure-report.md`, this index | Complete; product approvals pending |

## Commands executed

1. `sed -n '1,240p' /home/alex/.agents/skills/docs-context7-first/SKILL.md`
2. `wc -l AGENTS.md docs/AGENTS.md docs/recon/AGENTS.md docs/recon/*.md`
3. Bounded `sed -n '1,999p'` reads of `AGENTS.md`, `docs/AGENTS.md`, `docs/recon/AGENTS.md`, and every Markdown report under `docs/recon/`.
4. Target identity/content/toolchain inspection: `pwd`; Git probes; `rg --files`; bounded `find`; `rustc --version`; `cargo --version`; `python3 --version`.
5. Source identity/environment inspection: `pwd`; Git root/remote/branch/HEAD/log/status; `.venv/bin/python3 --version`; `uv --version`; package/test searches; SHA-256 of `uv.lock` and `pyproject.toml`.
6. Focused config-switch diagnostic under five isolated state roots with `timeout 45`, `faulthandler.dump_traceback_later(8, repeat=True)`, and no pytest cache: 1 passed in 1.43s.
7. `tests/test_agent.py` under isolated state and `timeout 300`: 208 passed in 6.13s.
8. Focused historical-environment comparison with `timeout 60`: 1 passed in 1.40s.
9. Full `tests/` under isolated state and `timeout 900`: 879 passed in 89.41s.
10. Post-suite process enumeration plus source HEAD/status and isolated-state inventory.
11. Context7 resolution/queries for official ACP Rust SDK, rusqlite, and reqwest documentation.
12. Current primary package metadata via `cargo search/info`, local rustup state, and source release-target searches.
13. `git init -b main`; target branch/status verification; no commit or remote.
14. Oracle module compilation and iterative bounded captures against the frozen source.
15. Two deterministic process-subset recaptures; complete fixture index matched byte-for-byte at SHA-256 `27e58c39fe95882961bf877b132b4ecbc6209850c57cd801fc2219e345632f86`.
16. `oracle.py validate-all` (65 scenarios) and `verify-index` (132 payload hashes); category counts and source process observations inspected.
17. ACP Context7/current package reconciliation; downloaded crate/schema/example/ordering inspection; exact-pinned `cargo fetch`.
18. ACP disposable spike `cargo test --locked`: 7 passed, 0 failed.
19. Rust SSE exact-pin resolution plus initial/final `cargo test --locked`: final 10 passed, 0 failed.
20. rusqlite package/feature and local SQLite probes; bundled `cargo test --locked` 6/6; system-feature test 6/6; feature and debug-binary size inspection.
21. Linux primitive probes: `command -v bwrap`; `bwrap --version`; `uname -a`; user-namespace sysctl; `unshare --user --map-root-user --pid --fork --mount-proc true`; bounded Bubblewrap PID/network tests.
22. Target/source searches for process, sandbox, Job Object, Seatbelt, process-group, and cancellation symbols using `git ls-files`, `rg`, and bounded numbered source reads.
23. Process spike dependency resolution and iterative `cargo test --locked`; the initial pinned `libc` conflict was corrected, a namespace assertion was corrected from host-backed `/sys` to `/proc/net/dev`, and an interrupted pipe-holder diagnostic was bounded and cleaned.
24. Explicit `kill 65198` of the sole synthetic fixture child left by the interrupted diagnostic, followed by survival and later process enumerations.
25. Final process spike `timeout 90s cargo test --locked`: process conformance 9/9, Linux Bubblewrap 3/3, no failures.
26. Official GitHub-hosted runner-label lookup from GitHub documentation; workflow matrix created for the five release-target families.
27. Final `oracle.py validate-all` (65) and `verify-index` (132), plus category-count calculation.
28. Final local spike matrix: exact-locked ACP 7/7, SSE 10/10, bundled SQLite 6/6, process 12/12, and system SQLite 6/6.
29. `cargo fmt` followed by `cargo fmt --check` for all four spikes; workflow parsed with PyYAML; macOS shell script parsed with `sh -n`; local `pwsh` availability probe (unavailable).
30. Completion searches for status markers/placeholders, fixture/report file counts, target Git status, source HEAD/status, and matching descendant processes.
31. Post-format exact-locked rerun of all local spike configurations; all passed. Final source HEAD remained `bf4d428…20045`, tracked diff count zero, and status retained only the pre-existing untracked roadmap prompt.
32. `cargo clean --manifest-path` for each disposable spike removed about 1.7 GiB of ignored build output without touching source or authored spike files.
33. Required-deliverable existence audit, incomplete-marker search, final source status, and process enumeration: no missing deliverables, incomplete markers, or fixture/oracle descendants.

## VRO-14 production completion acceptance — 2026-09-07

- Baseline `v0.20.88` / `9ff4695b`; requirement-to-source map and exact
  commands/results: `vro14-gap-audit.md`.
- Implemented real contained pipe-CDP sessions, render waterfall, bounded
  sitemap/gzip discovery, shared engine configuration and pinned driver build.
- Canonical verification passed; MSRV 1.88: 1,673 passed, 0 failed, 20 ignored
  across 87 suites; both unchanged release-profile performance gates passed.
- Immutable-image real browser and explicit navigation/chunked-fetch tests
  passed on Linux x86_64; no provider calls or user-state writes; post-test
  container enumeration empty. Two native image CI jobs gate public assets.
- DOX pass updated web/config/harness/sandbox, fixture, workflow and owning
  documentation contracts. Version-only crate contracts remain unchanged.
- Released `v0.20.89` at `5658da6eefa8a13042e938eaedccfdb7a1537ad5` after
  exact-commit canonical/supply-chain, MSRV, five-target and both driver-image
  jobs passed. Release run `34072500086` passed; all 16 public assets and seven
  archive digests verified, Linux binary version smoke passed, and the exact
  published x86_64 image passed both real-browser tests. Registry PR #539
  updated in place at fork commit `4f62da58cc37d6c77425218cc80fdc49db26f920`.

## VRO-15 independent acceptance audit — f662519

- `vro15-gap-audit.md` rejects the full-completion claim for implementation
  `fb14ea2`: 18 grouped findings with source evidence and proposed repairs.
  Production fixes await Alex's approval; default-off behavior remains unchanged.
- Reproduced workspace floors: 1,893 all-features / 1,869 default, zero failures.
  Actual swarm suite: 163 passing tests (also on Rust 1.88), not the ADR's 171.
- `vro15-audit-probes.rs`: 16 standalone probes reproduce defects in cancellation,
  pool bounds/replacement, topology failover, sandbox sharing/teardown, bus lifecycle
  and HNSW loading/determinism. Passing these probes confirms defects, not acceptance.
- Architecture, naming guard, workspace formatting and swarm Clippy pass. Full
  canonical/supply-chain/target-matrix acceptance is not certified by this audit.
- Treat the following original closeout as historical claims, not current acceptance.

## VRO-15 original closeout claims — 2026-09-10

- The original closeout described `vesper-swarm` as delivered in ten PRs (scaffold+guard, topology
  manager, worker pool, priority bus, assignment/timeout, HNSW core,
  hybrid ledger, sandbox leases, hive orchestrator + adapter,
  documentation closeout); decision record is
  `docs/adr/0025-provider-neutral-swarm-orchestration.md`, requirements
  and per-PR evidence live in `docs/swarm-oracle-extraction-prd.md`.
- Measured floors at close: **1,893 passed / 0 failed**
  (`cargo test --workspace --all-features`) and **1,869 / 0** (default
  features — the zero-degradation proof; the default build links no
  swarm symbols). Monotonic ladder across PRs: 1,741 → 1,767 → 1,785 →
  1,813 → 1,832 → 1,853 → 1,868 → 1,881 → 1,893; no test deleted or
  weakened.
- Quality bars with executable evidence: HNSW Recall@10 = 0.997 @ ef=16
  and 1.000 @ ef≥64 against brute-force cosine on 10k seeded vectors
  (`crates/vesper-swarm/tests/hnsw_tests.rs`); sandbox teardown survives
  holder panics with exact acquire/release pairing
  (`crates/vesper-swarm/tests/sandbox_tests.rs`); the naming embargo is
  a CI ratchet (`cargo xtask naming-guard`,
  `xtask/naming-guard-baseline.json`: 30 frozen pre-existing hits,
  0 new).
- `cargo xtask architecture` validates the feature-gated edges
  (`vesper-harness → vesper-provider/vesper-swarm` as optional deps
  only); `cargo xtask verify` runs the naming guard in CI.

## Outstanding acceptance

- Registry PR #539 awaits upstream review; the v0.20.89 release and web
  implementation acceptance are complete (`vro14-gap-audit.md`).

- Historical executor-stall cause is not reconstructable, but the complete source baseline is green and repeatable.
- Product approvals listed in `decision-register.md` and `blocker-closure-report.md`.
- ACP `PromptResponse.userMessageId` compatibility-wrapper detail during Stage 1.
- Linux ARM64, macOS Intel/Apple Silicon, and Windows x86-64 workflow execution.

## VRO-15 concurrent dispatch and embedding boundaries

- `crates/vesper-swarm/tests/hive_concurrency_regressions.rs`: three-party barrier
  overlap, prerequisite ordering, sibling cancellation and aliased-port capacity.
- `hive_boundary_regressions.rs`: hanging embedding is bounded, publishes nothing,
  retains the interrupted goal and refuses replay.
- Source/limits: `vro15-repair-execution.md`, concurrent-dispatch milestone.
  Native host, worker factory and lease/health acceptance are not implied.

## VRO-15 lease resource bounds

- `sandbox_tests.rs`: specification/timeout/boundary bounds refuse before backend
  work; 4,096 waiter overflow leaves state unchanged; dropped waiters reclaim the
  queue; shared joins cannot bypass 4,096 global members.
- `sandbox.rs` also bounds retained diagnostic bytes and guards identity exhaustion.
  Synchronous port preemption and real supervisor lifecycle acceptance remain open.

## VRO-15 local verification after concurrent waves and lease bounds

- `cargo test --workspace --all-features`: **1,954 passed, 0 failed, 21 ignored**.
- Workspace all-target/all-feature Clippy and Rust 1.88 locked compilation,
  formatting, architecture, naming and whitespace checks pass.
- Seven added regressions; feature remains default-off. Independent pool creation,
  supervisor/host integration and final audit acceptance remain explicitly open.

## VRO-15 independent factory milestone

- `pool_instance_regressions.rs`: seven passing tests for independent boot/turn
  overlap, growth/replacement/shrink, rollback, timeout/close/drop cancellation,
  alias refusal and failed-lease quarantine.
- `swarm_adapter_tests.rs`: seven passing adapter tests including native factory
  pooling followed by real tool execution and provider continuation.
- Hive per-instance lifecycle/lease/health wiring and host acceptance remain open;
  see `vro15-repair-execution.md`. No full F01/F07 completion claim is made.

## VRO-15 factory verification checkpoint

- Workspace all-feature tests: **1,962 passed, 0 failed, 21 ignored**.
- All-target/all-feature Clippy and Rust 1.88 locked compilation, formatting,
  architecture, naming guard and whitespace checks pass.
- Replacement physical-resource teardown remains a lease-composition gap;
  transactional pool publication does not prove supervisor capacity bounds.

## VRO-15 exact selected-lease execution

- `pool_selected_lease_regressions.rs`: three passing tests for exact selected
  identity, foreign/failed lease refusal and capability/deadline rejection.
- `run_task` and `run_leased_task` share the same execution boundary. Hive still
  needs concrete pool worker routing/provenance and lease/health/shutdown wiring.

- Selected-lease checkpoint: workspace **1,965 passed, 0 failed, 21 ignored**;
  all-target/all-feature Clippy and Rust 1.88 locked compilation, formatting,
  architecture, naming and whitespace checks pass. Full acceptance remains open.

## VRO-15 directed dispatch, bus correlation and source-captured scorer

- Disconnected dispatch and identical-prompt bus substitution regressions failed
  before repair. Candidate routing now follows live directed topology paths from
  the elected navigator; dispatch checks its exact message ID and concrete peers.
  Native three-session tool/synthesis integration still passes all four topologies.
- Seventy-two scoring outputs captured from the pinned oracle method match Rust;
  commit, method hash, type-match mapping and capture recipe are recorded in
  `vro15-repair-execution.md`. Oracle checkout remains unchanged and clean.
- Final canonical and Rust 1.88 workspace: **2,031/0/23**; default **1,994/0/17**.
  Scoped asynchronous leases, native Settings/hosts and external isolation/target
  acceptance remain open; no activation claim changes.

## VRO-15 backend teardown, identity and governance continuation

- Two backend regressions reproduced false-success teardown; explicit namespace/
  Docker cleanup now reports ownership, status and reaping failures. Post-deadline
  reaping is polled for at most 500 ms. Actual isolation remains platform-gated.
- Three lease identity regressions reproduced duplicate active/queued/quarantined
  admission; typed refusal now precedes backend/queue mutation.
- Strict counted JSON naming baseline preserves all 30 HEAD-frozen exceptions;
  four self-tests enforce stable line shifts without permitting new duplicates.
  ADR 0026 supersedes original completion/adapter/ACP-exclusion claims; current
  migration and PRD status correctly retain default-off repair status.
- ETXTBSY fixture failure reproduced; immutable checked-in CLI with isolated state
  replaces runtime-written executables. Twenty repeated fixture suites pass.
- Canonical and Rust 1.88 full workspace **2,027/0/23**, default **1,990/0/17**.
  Six namespace test bodies skip locally; three Docker integration tests are ignored.
  Detailed methods/limits and remaining acceptance: `vro15-repair-execution.md`.

## VRO-15 native composition and shared sandbox outcome acceptance

- Shared TUI/ACP sandbox adapter now reports teardown failure, retains run
  diagnostics and quarantines subsequent provisioning. Six outcome/refusal unit
  tests pass; actual supervisor failure/panic/hang acceptance remains open.
- Native Hive integration covers three barrier-overlapping isolated provider
  sessions, real read-file tool continuations, evidence-fed synthesis and no
  implicit durable state under all four topology configurations.
- Canonical offline verification and full Rust 1.88 tests pass **2,016/0/23**;
  default workspace **1,981/0/17**. Supply-chain gates and real Chrome interview
  browser test pass. One earlier MSRV driver-fixture spawn failure did not recur;
  OS diagnostics improved but its root cause remains unproven.
- `vro15-repair-execution.md` now contains the current F01–F18 reconciliation,
  exact acceptance limits and remaining native host/supervisor/platform work.
  Full repair is still in progress; activation remains default-off.

## VRO-15 explicit quarantine recovery

- `LeaseBook::retry_quarantined` adds bounded caller-owned cleanup through an
  explicitly supported idempotent backend port; unsupported/failed attempts keep
  capacity quarantined. Three offline regressions cover handoff, accounting,
  shared names, budgets and closed-book recovery.
- Package verification: **260 passed, 0 failed, 3 ignored**; strict package
  Clippy, Rust 1.88 locked all-target check, architecture, naming and whitespace
  checks pass. Native supervisor cleanup and host activation remain unaccepted;
  details and limitations are in `vro15-repair-execution.md`.

## VRO-15 concrete Hive pool routing

- Six `hive_pool_regressions.rs` tests: actual 1+3 pool overlap and executing-ID
  provenance, cross-role alias refusal, nominal-slot refusal, actual cancellation,
  notification ordering and transactional/idempotent topology admission.
- `Hive::with_factories` uses concrete pool instances; `Hive::new` no longer
  fabricates several topology workers from a single supplied port.
- Health-driven lifecycle changes, verified leases/teardown and native host
  acceptance remain open; details are in `vro15-repair-execution.md`.

- Concrete Hive routing checkpoint: workspace **1,971 passed, 0 failed, 21 ignored**;
  all-target/all-feature Clippy and Rust 1.88 locked compilation, formatting,
  architecture, naming guard and whitespace checks pass. Native host/full audit
  acceptance remains open; activation stays default-off.

## VRO-15 native hosts and shared scopes (active working tree)

`vro15-repair-execution.md` records local ACP transport and TUI command/task/history
acceptance, real configured loopback embeddings, native tool continuations, both
scope modes, shared-container sibling confinement and descendant cleanup, original
timestamps, portable snapshots and repaired clustered HNSW recall. Canonical/MSRV each pass 2072 tests; default workspace passes 2015. All four
optimized scale tests and final audit/deny pass. External exact-commit target and
namespace gates remain separately tracked for v0.21.6; v0.21.5 CI does not certify
this implementation. The TUI fixture's rejected synthetic-key public
request is recorded explicitly and excluded from acceptance; corrected runs assert
the configured loopback endpoint before dispatch.


## VRO-15 exact-commit supervisor follow-up

Initial candidate CI exposed the post-unshare overflow-ID defect, rootful Docker
artifact ownership mismatch and noncanonical macOS/Windows fixture roots. The
repair ledger and ADR 0027 record fixes, including private-root confinement,
capability drops, bounded pipe/handshake waits and truthful nonzero shell exits.
Real local namespace Hive and the explicit security/timeout gate now pass, as does
shared Podman confinement and original-owner restoration. Candidate CI success
must be re-established on the final commit before tagging; no release was used to
discover these failures.


## VRO-15 final implementation and release evidence

The final F01–F18 matrix records completed implementation and local acceptance,
including the corrected namespace supervisor. Canonical/MSRV: 2072 passed, zero
failed, 34 explicit ignored bodies; default: 2015/0/20. The required real namespace
and shared-container gates execute separately. Candidate `e6476df` has successful
canonical, MSRV and both-architecture complete web-driver workflows; its Windows
platform interruption was an HTTP 500 cache download before tests. Optional cache
setup now reaches the existing direct-compilation fallback. Final publication still
requires all four successful push workflows on the exact release commit. The
v0.21.6 release notes own the final run links, image IDs and local-install receipt.

## Native completion assurance implementation — 2026-09-11

- Approved ADR 0028 and `completion-assurance-execution.md` own the native gate,
  adversarial/real-repair cases, exact-case CI and bounded mutation evidence.
- Final repository verification and release readiness are reported there;
  research references alone do not establish implementation completion.
- Final canonical verification and both native binary builds passed. All 20
  exact acceptance cases passed; both deliberate evaluator mutations were caught.
  Local implementation evidence does not imply a release or installed update.

## VRO-16 release v0.21.9 — 2026-09-12

- Tag `v0.21.9` at `0b4cd0d`. The first version commit `b790e4f` was
  rejected by the web-driver exact-commit gate: the native loopback
  providers scripted only pre-governance turn shapes, so the composed
  review-panel and decision turns failed inside the native
  shared-service/ACP/TUI gates. This is the exact-commit contract
  working as designed — the gap was invisible to local canonical gates
  because the native container/service fixtures run only in the
  web-driver workflow's environment.
- Repair: `TaskKind::Review` maps to an empty tool registry in the
  provider adapter (D3 — judges evaluate, never author/re-execute),
  native fixtures answer review and decision turns, count expectations
  updated, cancellation invariant made baseline-relative. All gates
  reproduced locally with the real CI-built driver image (podman) and
  the release supervisor before pushing the fix.
- Green on the exact commit before tagging: canonical, MSRV,
  five-target foundation, dual-architecture web-driver (run ids
  34674795540/34674795595/34674795554/34674795541). Release run
  34676270024 verified exact-commit CI and published 16 assets;
  checksums verified locally; installer upgrade preserves user state;
  registry PR #539 updated in place.

## VRO-16 advanced hive governance — 2026-09-12

- Requirements: `docs/advanced-hive-governance-prd.md` (PR-1 §2, PR-2 §3,
  PR-3 §4). Upstreams referenced exclusively as governance alpha/beta
  (recon `docs/architecture/recon_vro16_governance.md`, pinned commits).
- Final audit found nine integration gaps (G1–G9) between the tested
  engines and the composed production path — the VRO-15 decorative-layer
  failure mode. All were repaired in the same audit pass and each now has
  an executing proof in
  `crates/vesper-swarm/tests/hive_governance_audit_fixes.rs`.
- G2 gate bus publication is real: the `governor` inbox subscribes at
  topology admission (`MessageKind::Governance`, Urgent tier); gate sends
  fail loudly on publication failure; `drain_governance_bus` is the host
  observation seam.
- G3 ledger audit trail is complete: every gate resolution persists
  before its state effects (Cancel no longer drops its own audit record);
  `record_gate_event` derives the goal from the event, and the audit
  entry round-trips as an `AuditEvent` through `EntryKind::Audit`.
- G4 `governance: gated` enforces decomposition and synthesis boundary
  gates (once per task id per run — `resolved_gate_ids`).
- G5 Redirect directives reach re-dispatched worker prompts.
- G9 SmartPause consumes the real watchdog percentage
  (`BudgetWatchdog::percent_consumed`), not a constant.
- G1 the review panel composes pre-synthesis on governance-enabled hives
  (async round driver over driver ports; bare VRO-15 hives keep exact
  turn-count contracts; zero-panel fails closed).
- G6 budget exhaustion renders in both hosts; G7 the directive rides
  `GateResolved.command` (PRD D4 amended instead of a redundant variant);
  G8 this entry.
- **G4 re-audit (same day, prompted by direct owner challenge):** the
  boundary-gate fix above was itself incomplete — gates opened but did
  not pause the gated work (the decomposition gate fell through to task
  dispatch in the same tick; the synthesis gate ran panel+synthesis
  anyway), the G4 proof asserted only gate *views*, not held-back work,
  and `run_to_completion` could hot-loop on an unresolvable open gate.
  All three repaired: both boundaries now park the run (`Ok(false)`,
  `active_goal` cleared, assignments/evidence retained) and resume after
  host resolution or expiry; `run_to_completion` returns instead of
  spinning when a gate is open; the proof is behavior-based — zero task
  prompts reach drivers while the decomposition gate is open, synthesis
  is held until its gate resolves, the goal completes after both
  resolutions, and the spin-safety case asserts the run stays parked.
  Lesson recorded: a gate that does not stop anything is not a gate, and
  proving "the gate exists" is not proving "the gate gates."
- Final verification: workspace all-features and default suites, strict
  Clippy, fmt, architecture (27 packages), naming-guard (11 tokens, 18
  frozen hits, zero growth), `cargo xtask acceptance` 20/20. Counts in
  `docs/migration-status.md` VRO-16 row.

- `v0.22.1-release-execution.md` — exact-commit release of the A1 audit fix: 4 workflows green on 1b221dc, tag v0.22.1, 16 assets, binary 0.22.1, registry PR #539 head 8138e28, local install updated (both binaries 0.22.1).
- `ranker-hardening-final-audit.md` — cross-PR audit: scope fence verified (2-line diff), pin sabotage-verified bidirectionally, A1 latent metric defect found+fixed (overhead tokenizer vs ranker pool; shipped numbers unaffected), M2 re-derived, release evidence reconciled.
- `ranker-hardening-pr3-execution.md` — stop rule clean (canonical table + overheads identical under the hardened ranker; verdict re-derived ADOPT); v0.22.0 released per the exact-commit contract (4 workflows green on 9b56a9e; tag; 16 assets; binary 0.22.0; registry PR #539 updated in place).
- `ranker-hardening-pr2-execution.md` — Option A decoupling: chunk pools raw-stemmed (`raw_semantic_tokens`), prompt pool keeps expansion; pin green, control green, D3 ladder green, skill tier frozen, floor 2,220.
- `ranker-hardening-pr1-execution.md` — Option D anchor: cross-talk pin fails for the audited reason (rollback displaces migrate via 1,560-pt manufactured overlap); no-alias control green; floor 2,219; `#[ignore]` until PR-2.
