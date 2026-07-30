# Stage 3 Evidence Index

Status: COMPLETE

Frozen source commit:
`bf4d4287e2e3320aa3f09015f678e6169d520045`

## Preflight

| Item | Confirmed state |
| --- | --- |
| Target | `main`; no remote; no initial commit; existing project content untracked |
| Source | Exact frozen HEAD; tracked diff empty; only pre-existing `docs/codex-tui-roadmap-prompt.md` untracked |
| Workspace | Seven production/foundational crates plus `xtask`; GLM is a leaf adapter |
| Fixtures | 76 scenarios / 154 indexed payloads |
| Fixture index SHA-256 | `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a` |
| Stage 2 tests | 63 passing on Rust 1.95 and 1.88 |
| Local platform | Linux x86-64 validated; four target families remain CI pending |

All root and applicable scoped `AGENTS.md`, Stage 2 reports, accepted ADRs,
architecture/dependency records, GLM reconnaissance, behavioral/security/parity
reports, fixture manifests/results, focused frozen-source modules/tests, and the
disposable SSE spike were inspected before implementation.

Context7 was attempted first for the HTTP/runtime dependency APIs, but the
connected service reported its monthly quota exhausted. Pinned crate metadata
and local source documentation confirmed:

- `reqwest 0.13.4`: MSRV 1.85, Rustls feature, explicit timeout/redirect/proxy/
  retry/stream builder surface;
- `tokio 1.52.0`: MSRV 1.71;
- `tokio-util 0.7.17`: MSRV 1.71;
- `bytes 1.12.1`: MSRV 1.57;
- `httpdate 1.0.3`: MSRV 1.56.

No credential file was read.

## Frozen GLM evidence

- Endpoints/catalog/retry/generation/reasoning constants:
  `glm_acp/config.py:415-475,495-633`.
- Credential precedence and legacy stored-key behavior:
  `glm_acp/config.py:669-777`; `tests/test_config.py:586-634`.
- Client trust/auth/cancellation:
  `glm_acp/glm_client.py:111-167`.
- Continuation wording/cap/preserved reasoning:
  `glm_acp/glm_client.py:169-230`.
- Bounded auxiliary requests:
  `glm_acp/glm_client.py:232-371`.
- Official-host quota normalization:
  `glm_acp/glm_client.py:373-471`;
  `tests/test_glm_client.py:133-282`.
- Request, retry, SSE, usage, and tool assembly:
  `glm_acp/glm_client.py:519-801`;
  `tests/test_stream_integration.py`.
- Session-to-client compatibility mapping:
  `glm_acp/agent.py:709-753`.

## Command ledger

```text
pwd
git status --short
git branch --show-current
git remote -v
rg --files -g AGENTS.md
sed/nl/rg/find/jq over required DOX, Stage 2, architecture, ADR, fixture,
source, test, spike, and foundational contract files
git -C <SOURCE> rev-parse HEAD
git -C <SOURCE> status --short
git -C <SOURCE> remote -v
cargo tree --workspace --depth 1
sha256sum fixtures/manifest-sha256.json
cargo info reqwest@0.13.4
cargo info tokio@1.52.0
cargo info tokio-util@0.7.17
cargo info bytes@1.12.1
cargo info httpdate@1.0.3
cargo check --workspace --all-targets --all-features
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --doc
cargo xtask fixtures validate
cargo xtask fixtures verify-index
cargo xtask fixtures coverage --stage 3
cargo xtask contracts verify
cargo xtask provider glm verify
cargo xtask architecture
cargo xtask verify
cargo xtask msrv
cargo audit
cargo deny --all-features check
python3 -c <parse all workflow YAML>
rg forbidden placeholder/dependency/SDK/provider/secret patterns
git -C <SOURCE> rev-parse HEAD
git -C <SOURCE> diff --name-only
git -C <SOURCE> status --short
git status --short
```

## Phase ledger

| Phase | Status | Evidence |
| --- | --- | --- |
| Preflight/source inspection | COMPLETE | This index and exact citations above |
| Crate/catalog/config/auth/request | COMPLETE | Production modules and exact golden tests |
| Transport/SSE/stream/retry/cancel | COMPLETE | Bounded loopback and arbitrary-chunk tests |
| Continuation/quota/compatibility | COMPLETE | Exact continuation, isolated quota, read-only translation |
| Fixtures/coverage/conformance | COMPLETE | 76 covered; all 21 GLM source scenarios implemented |
| Governance/docs/CI | COMPLETE | Workspace, xtask, workflow, dependency and DOX updates |
| Final verification | COMPLETE | 88 tests on 1.95 and 1.88; all local gates pass |

## Changes

- Created `crates/vesper-provider-glm`, `fixtures/coverage-stage3.json`, and the
  Stage 3 report set.
- Updated workspace/lock/dependency policy, `xtask`, three workflows, current
  architecture/migration/README records, and applicable DOX documents.
- Added no fixture payload and made no frozen-source change.

## Unresolved

- Remote Linux ARM64, macOS Intel/Apple Silicon, and Windows x86-64 CI remains
  unexecuted.
- Live-provider interoperability was intentionally not exercised.

## Final proof

- 88 workspace tests and 25 GLM tests passed on Rust 1.95 and 1.88.
- Format, strict Clippy, check, docs, fixture/index, coverage, contracts,
  architecture, Cargo Audit, Cargo Deny, workflow parsing, and forbidden scans
  passed.
- Fixture corpus remains 76 scenarios / 154 payloads with index
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
- Source HEAD/diff/status remained invariant; target remains uncommitted on
  `main` with no remote.
