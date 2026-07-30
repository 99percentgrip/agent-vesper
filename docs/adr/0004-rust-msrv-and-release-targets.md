# ADR 0004: Rust MSRV and Release Targets

Status: ACCEPTED

## Context

Agent Vesper must retain the reference release families while keeping dependency
selection reviewable and reproducible.

## Decision

The MSRV is Rust 1.88 on stable. Development is pinned to stable 1.95.0 in
`rust-toolchain.toml`; a dedicated job verifies 1.88 separately. Required target
families are Linux x86-64, Linux ARM64, macOS Intel, macOS Apple Silicon, and
Windows x86-64.

Cross-compilation is not runtime validation. Platform capability reporting must
be honest, and required sandboxing fails closed when unavailable.

## Alternatives considered

- Unpinned stable: rejected because results drift.
- Current toolchain as MSRV: rejected as unnecessary churn.
- Reducing release targets: rejected by the approved compatibility policy.

## Consequences

Every direct dependency must declare an MSRV compatible with 1.88. Remote
five-target jobs remain required even when local Linux tests pass.

## Compatibility implications

The five source release target families remain mandatory; capability differences
are exposed rather than hidden.

## Security implications

Unsupported mandatory isolation cannot silently degrade. Platform-specific unsafe
code, if later needed, is isolated and reviewed.

## Migration implications

Dependency upgrades that exceed 1.88 require an ADR. Platform runtimes arrive only
at their owning stages.

## Verification requirements

Run formatting, Clippy, and tests on the pinned toolchain; build/test foundational
crates on 1.88; execute eligible tests on all five target families.

## Evidence

- Historical decision: [foundation ADR 0007](../foundation/adr/0007-rust-support-policy.md)
- Platform spike: [process/sandbox report](../foundation/process-sandbox-spike.md)
- Dependency record: [dependencies](../dependencies.md)
