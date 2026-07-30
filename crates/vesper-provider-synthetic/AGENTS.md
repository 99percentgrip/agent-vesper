# Vesper Synthetic reference provider

## Purpose

Own a deterministic, in-process reference provider adapter that proves the
`vesper-provider` contract is genuinely provider-neutral. It is the
multi-provider proof-of-concept alongside the production GLM adapter: a second,
independent adapter implements `ProviderFactory` and `ProviderSession` end to
end with no GLM, network, or secret dependency.

## Ownership

- `src/` owns the reference adapter behavior behind `vesper-provider` ports.
- `src/config.rs` owns the descriptor, configuration, catalog, and capabilities.
- `src/factory.rs` owns the `SyntheticFactory`.
- `src/session.rs` owns the `SyntheticSession` and its ordered stream.

## Local Contracts

- Depend only on `vesper-domain` and `vesper-provider` in production.
- No network, HTTP, authentication, secret, or GLM dependency.
- Emit exactly one terminal completion per turn and honor the ordered-stream
  invariant every concrete adapter must satisfy.
- Never reference `vesper_provider_glm` or any other concrete adapter.
- Capabilities are advertised honestly: bounded text output is native, and
  everything else stays `Unknown` rather than over-advertised.

## Work Guidance

- Keep the adapter deterministic so tests and integrations can rely on it.
- Prefer extending the descriptor/catalog over hardcoding behavior outside the
  configuration envelope.

## Verification

- Run `cargo test -p vesper-provider-synthetic`.
- Run strict workspace Clippy and architecture checks.

## Child DOX Index

No children.
