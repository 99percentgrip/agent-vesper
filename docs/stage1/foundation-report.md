# Agent Vesper Stage 1 Foundation Report

Status: COMPLETE LOCALLY — CI VALIDATION PENDING

## Verdict

The production Rust workspace foundation is complete on the local Linux x86-64
host. It is ready for the next bounded stage, subject to the explicitly pending
five-target CI execution. No production agent loop, concrete provider, ACP
runtime, session writer, tool executor, process backend, MCP, memory, automation,
plugin system, or TUI was implemented.

## Accepted ADRs

Eight accepted records under `docs/adr/` govern independent state roots and
legacy compatibility; reasoning retention; TUI behavioral/accessibility parity;
Rust 1.88 and five release families; argv versus shell execution; provider-neutral
core/adapters; session actors with hierarchical cancellation; and fixture-oracle
parity gates. Historical `docs/foundation/adr/` records remain intact.

## Workspace and graph

The workspace contains `vesper-domain`, `vesper-security`, `vesper-config`,
`vesper-provider`, `vesper-policy`, `vesper-testkit`, and `xtask`. The production
graph is acyclic:

```text
domain ← config → security
domain ← provider
domain ← policy → security
foundational contracts ← testkit ← xtask
```

Production crates do not depend on testkit, frontend, concrete-provider, ACP,
HTTP, SQLite, TUI, MCP, or spike code. Every current crate forbids unsafe code.

## Files created or updated

- Workspace/governance: `Cargo.toml`, `Cargo.lock`, toolchain/format/Clippy/Deny
  policy, `.cargo/`, `.gitignore`, root and scoped `AGENTS.md`.
- Production contracts: all files under the six authorized `crates/` directories.
- Maintenance: `xtask/` with verify, fixtures, architecture, and MSRV commands.
- Project docs: `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE`,
  architecture/workspace/dependency/migration documents, eight production ADRs,
  and this Stage 1 evidence set.
- Fixtures: only the generated `fixtures/coverage-stage1.json`; the authoritative
  manifests/results/schemas/hash index were not rewritten.
- CI: pull-request, MSRV, five-target foundation, and preserved/updated foundation
  spike workflows.

## Dependencies

Nine exact direct crates are justified in `docs/dependencies.md`. Cargo Audit
reported no known vulnerabilities. Cargo Deny passed advisories, licenses,
sources, and bans; it reports one reviewed warning because `jsonschema` currently
brings `syn` 2 while current derive dependencies use `syn` 3. There are no Git
dependencies, wildcard dependency requirements, HTTP clients, runtime SDKs, or
production database/UI dependencies.

## Fixtures

Rust parsed and schema-validated all 65 scenarios and verified all 132 indexed
payload hashes. Coverage is 12 foundational contracts implemented and 53 runtime
scenarios deferred. Event order and canary checks are enforced. Details are in
`fixture-coverage.md` and `fixtures/coverage-stage1.json`.

## Verification results

```text
cargo fmt --all --check                                      PASS
cargo clippy --workspace --all-targets --all-features
  -- -D warnings                                             PASS
cargo test --workspace --all-features                        PASS (44/44)
cargo test --workspace --doc                                 PASS
cargo xtask fixtures validate                                PASS (65)
cargo xtask fixtures verify-index                            PASS (132)
cargo xtask architecture                                     PASS (7 packages)
cargo xtask verify                                           PASS
cargo xtask msrv                                             PASS (Rust 1.88.0, 44/44)
cargo audit                                                  PASS
cargo deny --all-features check                              PASS (1 reviewed duplicate warning)
PyYAML parse of .github/workflows/*.yml                      PASS (4 files)
```

No live provider or user-state operation was performed. Build/test measurements
are foundation engineering data only and are not evidence of runtime improvement.

## Platform status

Linux x86-64 is locally validated. Linux ARM64, macOS Intel, macOS Apple Silicon,
and Windows x86-64 have real jobs configured but remain CI-validation pending.
Prepared platform spikes are never reported as passed without their hosts.

## Source invariance and target status

The final source audit confirmed the frozen commit, zero tracked changes, and only
the pre-existing untracked `docs/codex-tui-roadmap-prompt.md`. No source command
wrote to the reference repository. The target remains on `main`, has no remote,
and is intentionally uncommitted under the repository’s commit policy. Exact
`git status --short` is retained in the mission command log and final handoff.

## Deferred work and risk

- Five-target workflows have not yet executed; failures are release-blocking.
- Persistence-complete session fields and legacy imports belong to their owning
  stage.
- Runtime cancellation hierarchy and the fixed descendant cleanup guarantee need
  production implementations and race/conformance tests later.
- Fixture validation depends on a comparatively broad test-only `jsonschema`
  tree; duplication remains reviewed via Cargo Deny.
- GLM parity and every other provider remain unimplemented.

READY FOR STAGE 2 WITH CI VALIDATION PENDING
