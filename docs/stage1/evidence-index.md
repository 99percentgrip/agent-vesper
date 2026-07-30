# Stage 1 Evidence Index

Status: COMPLETE

Frozen source commit:
`bf4d4287e2e3320aa3f09015f678e6169d520045`

## Ledger

| Area | Evidence | Result | Remaining validation |
| --- | --- | --- | --- |
| Authoritative inputs | All required `docs/recon/`, `docs/foundation/`, ADR, fixture, and scoped `AGENTS.md` records read before edits | Confirmed | None |
| Accepted decisions | `docs/adr/0001` through `0008` | Eight accepted production ADRs | Future decisions remain stage-owned |
| Workspace | Root `Cargo.toml`, `Cargo.lock`, six crates, `xtask/` | Resolves; graph acyclic | Remote CI pending |
| Domain | `crates/vesper-domain/src/`; 10 tests | Typed IDs/content/usage/finish/errors/events, reasoning modes, and extensions pass | Persistence-complete session schema deferred |
| Provider | `crates/vesper-provider/src/`; 5 tests | Capability, fallback, stream terminal, cancellation, metadata, no-replay contracts pass | Concrete GLM and transports deferred |
| Configuration | `crates/vesper-config/src/`; 6 tests | Injected Linux/macOS/Windows paths, profiles, legacy descriptors, secret references pass without state writes | Real-host non-Linux validation pending |
| Security | `crates/vesper-security/src/`; 8 tests | Secret/reference, URL/env redaction, untrusted context, bounds, paths/isolation pass | OS sandbox/process backends deferred |
| Policy | `crates/vesper-policy/src/lib.rs`; 7 tests | Six-case fixture matrix and precedence invariants pass | Approval transports/integration deferred |
| Testkit | `crates/vesper-testkit/src/`; 8 tests | Schema/index/order/canary/normalization/fakes pass | Runtime comparisons deferred by coverage map |
| Fixtures | `fixtures/coverage-stage1.json` | 65 parsed and schema validated; 132 hashes match; 12 foundational/53 deferred | Owning-stage parity |
| Architecture | `cargo xtask architecture` | Seven-package allowlisted graph passes; forbidden reference scan passes | Re-run in CI |
| MSRV | `cargo xtask msrv` | Rust 1.88.0: 44 tests pass | CI rerun pending |
| Supply chain | `cargo audit`; `cargo deny --all-features check` | Advisories/licenses/sources/bans pass; reviewed `syn` 2/3 duplication warns | CI rerun pending |
| Workflows | Four `.github/workflows/*.yml` files parsed with PyYAML | YAML locally valid | No workflow has executed remotely |
| Source invariance | Final Git identity/status audit | Exact commit, zero tracked changes, expected untracked roadmap only | None |

## Principal commands

```text
cargo check --workspace --all-targets --all-features
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --doc
cargo xtask fixtures validate
cargo xtask fixtures verify-index
cargo xtask architecture
rustup toolchain install 1.88.0 --profile minimal
cargo xtask msrv
cargo audit
cargo install cargo-deny --locked --version 0.20.2
cargo deny --all-features check
```

The authoritative completion output and final repository status are recorded in
[foundation-report.md](foundation-report.md).
