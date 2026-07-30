# Stage 2 Evidence Index

Status: COMPLETE

Frozen source commit:
`bf4d4287e2e3320aa3f09015f678e6169d520045`

## Starting state

| Item | Confirmed preflight |
| --- | --- |
| Target | `main`; no remote; no initial commit; all project content untracked |
| Source | Exact frozen HEAD; tracked diff empty; only pre-existing `docs/codex-tui-roadmap-prompt.md` untracked |
| Workspace | Six foundational crates plus `xtask`; acyclic |
| Dependencies | Nine direct dependencies; no Stage 2 addition |
| Baseline tests | 44 Stage 1 tests on Rust 1.95/1.88 |
| Baseline fixtures | 65 scenarios, 132 payloads, index `27e58c39fe95882961bf877b132b4ecbc6209850c57cd801fc2219e345632f86` |
| Baseline coverage | 12 implemented foundational scenarios; 53 deferred |

All required root/scoped `AGENTS.md`, Stage 1, foundation, reconnaissance,
architecture, dependency, migration, ADR, fixture, oracle, ACP, and SSE records
were read before implementation.

Context7 was attempted for Serde version-sensitive behavior; its monthly quota
was exhausted. The offered API-key file was not read. Pinned local crate source
and executable decode/encode tests supplied the needed evidence.

## Phase ledger

| Phase | Result | Durable evidence |
| --- | --- | --- |
| Preflight | COMPLETE | repository/source identity and baseline above |
| 53-fixture audit | COMPLETE | `fixtures/coverage-stage2-plan.json` |
| Domain contracts | COMPLETE | IDs, versions/extensions, content/tools/usage/errors, commands/events, compatibility |
| Provider contracts | COMPLETE | capabilities, request validation, ports, continuation, stream/error state |
| Session compatibility | COMPLETE | all seven outcomes tested; `session-v1-compatibility.md` |
| ACP-neutral map | COMPLETE | `acp-contract-map.md` |
| Oracle expansion | COMPLETE | 11 deterministic synthetic vectors; `oracle-expansion-report.md` |
| Testkit/coverage | COMPLETE | conformance assertions/fakes; 76-scenario map |
| Documentation/CI | COMPLETE | Stage 2 reports, root docs, three workflows updated |
| Verification | COMPLETE LOCALLY | 63 tests on Rust 1.95 and 1.88; strict checks pass |

## Fixture changes

- The 65 source-derived scenario pairs were not regenerated or edited.
- Added 11 explicitly synthetic provider-neutral contract vectors.
- Extended schema enums with `contracts` and `contract-generator`.
- Final corpus: 76 scenarios and 154 indexed payloads.
- Final index SHA-256:
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- Two independent synthetic generations produced
  `d51f733180d2ae3c6d76f42827656e412936590cde1cbe27a6e5e9e5460fd8da`;
  recursive diff was empty.

## Dependency changes

None. `Cargo.toml` and the resolved dependency set did not change.

## Commands executed

Repository/instruction/evidence inspection:

```text
git branch --show-current
git status --short
git remote -v
git -C <SOURCE> rev-parse HEAD
git -C <SOURCE> status --short
git -C <SOURCE> diff --name-only
find . -name AGENTS.md -not -path ./target/*
find/rg/wc/sed/cat/jq/nl over the required reports, fixtures, source symbols,
workspace manifests, crate sources, workflows, and scoped instructions
cargo metadata --format-version 1 --no-deps
cargo tree --workspace --edges normal --depth 1
sha256sum fixtures/manifest-sha256.json
```

Implementation and focused checks:

```text
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features --no-fail-fast
cargo test -p vesper-testkit fixture::tests::all_seven_session_fixture_outcomes_have_compatibility_coverage -- --exact
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask fixtures coverage --stage 2
cargo xtask contracts verify
cargo xtask architecture
cargo xtask fixtures validate
cargo xtask fixtures verify-index
```

Oracle/fixture determinism:

```text
mktemp -d /tmp/vesper-contract-a.XXXXXX
mktemp -d /tmp/vesper-contract-b.XXXXXX
python3 tools/python-oracle/generate_contract_fixtures.py --output <tmp-a>
python3 tools/python-oracle/generate_contract_fixtures.py --output <tmp-b>
diff -ru <tmp-a> <tmp-b>
python3 tools/python-oracle/generate_contract_fixtures.py --output fixtures/contracts
python3 tools/python-oracle/oracle.py rebuild-index
```

Final governance:

```text
cargo xtask verify
cargo xtask msrv
cargo audit
cargo deny --all-features check
python3 -c <parse every .github/workflows/*.yml with PyYAML>
rg <TODO/FIXME/todo!/unimplemented!/ignored-test/fake-success scan>
rg <forbidden production dependency and SDK/HTTP/database/frontend/I/O scan>
git -C <SOURCE> rev-parse HEAD
git -C <SOURCE> diff --name-only
git -C <SOURCE> status --short
git branch --show-current
git remote -v
git status --short
```

## Verification results

- Rust 1.95: 63 unit tests passed; 0 failed/ignored; doc tests passed.
- Rust 1.88: the same 63 tests passed; 0 failed/ignored.
- Formatting and strict all-target/all-feature Clippy passed.
- Fixture schema/index/coverage/contract gates passed.
- Architecture gate passed for seven packages.
- Cargo Audit reported no vulnerability.
- Cargo Deny passed advisories, bans, licenses, and sources; it reports the
  reviewed transitive `syn` 2/3 duplicate warning.
- Four workflow YAML files parsed.
- Forbidden placeholder, ignored-test, dependency, SDK, transport, database,
  frontend, and domain-I/O scans were empty.

## Final source/target state

Source HEAD remains exact; tracked diff is empty; the sole untracked roadmap
file is unchanged. Target remains on `main`, has no remote and no commit, and
therefore all repository files remain untracked. No provider, ACP runtime, core
loop, executor, persistent store, or frontend crate was created.

## Unresolved/external

- Linux x86-64 is locally validated.
- Linux ARM64, macOS Intel, macOS Apple Silicon, and Windows x86-64 remain
  unexecuted CI jobs.
- Runtime GLM transport/cancellation/retry behavior belongs to Stage 3.

