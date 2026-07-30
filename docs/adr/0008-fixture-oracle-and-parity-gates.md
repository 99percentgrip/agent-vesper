# ADR 0008: Fixture Oracle and Parity Gates

Status: ACCEPTED

## Context

Migration safety depends on observable behavior rather than Python-shaped Rust.
The frozen reference produced 65 deterministic language-neutral scenarios and a
hash-indexed canonical corpus.

## Decision

The frozen source commit is the behavioral oracle. Fixture manifests/results and
their SHA-256 index are immutable unless a schema defect is demonstrated and
reviewed. Every stage records each scenario as parsed, schema validated,
foundational contract implemented, or deferred to its owning stage.

Exact-output, semantic, schema-compatibility, security-invariant, and performance
comparisons remain distinct. Normalization is literal and allowlisted; event
order, linkage, outcomes, hashes, redaction, and cancellation categories are
never normalized away. Intentional behavioral changes need an ADR and explicit
fixture treatment.

## Alternatives considered

- Hand-written Rust-only expected values: rejected because they can encode the
  implementation’s own mistakes.
- Snapshot everything as exact text: rejected because some behavior is semantic.
- Regenerate fixtures on every build: rejected because it destroys the baseline.

## Consequences

Rust validates all 65 scenarios and 132 indexed payloads from the repository.
Runtime fixtures remain deferred until their owning stages rather than faked.

## Compatibility implications

The source command-cancellation leak is a negative security fixture: parity means
Rust is at least as safe, not that it reproduces the defect.

## Security implications

Canary scanning is mandatory. Fixture runners use isolated state, no credentials,
bounded timeouts, and process-tree cleanup.

## Migration implications

Every stage updates `fixtures/coverage-stage1.json` or its successor and must pass
its declared parity gate before expanding scope.

## Verification requirements

Schema validation, index verification, normalization collision tests, order
checks, canary checks, and applicable DTO round trips must run in CI.

## Evidence

- [fixture charter](../foundation/fixture-charter.md)
- [oracle report](../foundation/python-oracle-report.md)
- [parity strategy](../recon/parity-test-strategy.md)
- Corpus: `fixtures/`
