# ADR 0006: Provider-Neutral Core and Provider Adapters

Status: ACCEPTED

## Context

The proven harness contains shared behavior plus GLM-specific request, streaming,
retry, and capability behavior. Duplicating the agent loop per provider or
reducing every provider to booleans would make future expansion unsafe.

## Decision

Agent Vesper has one provider-neutral agent loop. Provider adapters depend on
provider contracts but never on the future core engine. Capabilities are typed as
`Native`, `Emulated`, `Unsupported`, or `Unknown`; requests express `Require`,
`Prefer`, or `AllowFallback`. Required unsupported features fail before dispatch,
and fallbacks are observable.

Provider SDK and ACP SDK types do not enter domain DTOs. Provider extensions are
namespaced opaque values. Streaming guarantees ordered normalized events and one
terminal outcome or classified error.

## Alternatives considered

- Provider conditionals in core: rejected due to hidden coupling.
- One giant provider trait: rejected because it freezes unrelated operations.
- Lowest-common-denominator booleans: rejected because advanced capabilities and
  fallback semantics disappear.

## Consequences

Small ports own catalogs, factories, and sessions. Concrete providers arrive only
after GLM parity stages authorize them.

## Compatibility implications

GLM-specific finish, reasoning, tool delta, usage, and retry details remain
representable without making them universal.

## Security implications

Providers cannot execute tools, decide policy, access raw secrets, or acquire
filesystem/process authority.

## Migration implications

GLM is the first production adapter. Multi-provider production expansion waits
for the GLM parity gate.

## Verification requirements

Architecture checks reject concrete provider/SDK/frontend leakage. Contract tests
cover support resolution, fallback observability, cancellation, terminal
uniqueness, and partial-output no-replay.

## Evidence

- [provider abstraction analysis](../recon/provider-abstraction-analysis.md)
- [Rust architecture proposal](../recon/rust-architecture-proposal.md)
- Stage 1 contracts: `crates/vesper-provider/`
