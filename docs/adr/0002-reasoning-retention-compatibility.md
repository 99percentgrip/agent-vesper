# ADR 0002: Reasoning Retention Compatibility

Status: ACCEPTED

## Context

The reference can retain GLM reasoning needed for display or continuation.
Provider-visible reasoning, summaries, opaque continuation records, and hidden
internal chain-of-thought are not the same data.

## Decision

Initial GLM parity preserves the reference default. The domain represents
`Persist`, `SessionOnly`, and `Disabled`, and distinguishes provider-visible,
summary, and opaque-continuation reasoning. Existing stored reasoning must
round-trip when imported. Hidden chain-of-thought is not a provider-neutral
requirement.

Reasoning is excluded from telemetry, indexes, generic logs, failure corpora,
worker prompts, hooks, cron artifacts, and default exports. Explicit eligible
exports remain future work. Secret and redaction policies apply equally.

## Alternatives considered

- Disable persistence immediately: rejected because it silently changes parity.
- Treat all reasoning as normal text: rejected because it loses privacy and
  continuation semantics.
- Require chain-of-thought from every provider: rejected as unsafe and invalid.

## Consequences

Retention and reasoning kind are explicit domain data. Sinks must opt in rather
than inheriting reasoning accidentally.

## Compatibility implications

Legacy readers preserve existing records. Changing the default requires a later
privacy ADR and migration UX.

## Security implications

Opaque or visible reasoning is sensitive content and cannot enter general sinks.
Provider-private metadata remains namespaced and bounded.

## Migration implications

Provider adapters will map their distinct reasoning forms. Persistence and export
stages must enforce retention independently.

## Verification requirements

Round-trip tests cover kind and retention. Later sink conformance tests must prove
redaction and exclusion under every retention mode.

## Evidence

- Historical decision: [foundation ADR 0002](../foundation/adr/0002-reasoning-persistence.md)
- Security contract: [security invariants](../recon/security-invariants.md)
- Fixtures: `fixtures/sessions/v1/reasoning-*`
