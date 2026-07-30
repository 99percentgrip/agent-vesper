# Foundation Evidence Index

Status: COMPLETE

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

## Open questions

- Historical executor-stall cause is not reconstructable, but the complete source baseline is green and repeatable.
- Product approvals listed in `decision-register.md` and `blocker-closure-report.md`.
- ACP `PromptResponse.userMessageId` compatibility-wrapper detail during Stage 1.
- Linux ARM64, macOS Intel/Apple Silicon, and Windows x86-64 workflow execution.
