# Z.ai GLM provider adapter

## Purpose

Own production GLM provider identity, catalog, endpoint/authentication policy,
wire serialization, HTTP/SSE transport, retry/cancellation, continuation,
quota normalization, and legacy GLM compatibility translation.

## Ownership

- `src/` owns production adapter behavior behind `vesper-provider` ports.
- `tests/` owns deterministic loopback integration and fixture conformance.

## Local Contracts

- Depend only on `vesper-auth`, `vesper-domain`, `vesper-provider`,
  `vesper-config`, and `vesper-security` in production.
- `vesper-testkit` is permitted only as a dev dependency.
- Use no live provider calls or real credentials in tests.
- Bound every wire line, event, tool field, metadata value, and error prefix.
- Credentials are attached only at dispatch and never enter normal formatting,
  serialization, errors, events, or fixtures.
- Do not expose HTTP or GLM wire types through neutral provider ports.
- Emit no event after terminal completion or cancellation.
- Never retry after visible output.

## Work Guidance

- Keep exact GLM wire compatibility inside this crate.
- Keep environment-variable precedence and legacy credential reads, but route
  all new stored credentials through `vesper-auth`.
- Use loopback ephemeral servers and synthetic credential sources in tests.
- ADR 0009: advertise **one** session-scoped reasoning dial (`zai:reasoning`,
  alias `thinking`, scale `{disabled, enabled, high, max}`). The former
  separate `zai:effort` and `zai:interleaved-thinking` controls are retired;
  `low`/`medium` are invalid. `reasoning_mode_for_superpower` maps a resolved
  `SuperpowerValue` into the runtime reasoning-mode label; `serialize_request`
  already turns `request.reasoning.mode` into the wire `reasoning_effort` /
  `thinking` pair.
- Model registry lives in `catalog.rs` (`FrozenModel` table). `glm-5.3` is the
  current flagship and the adapter default; deep reasoning (`high`/`max`) gates
  on the flagship line via `catalog::supports_deep_reasoning`
  (`glm-5.3` + `glm-5.2`). Loopback conformance fixtures are captured against
  `glm-5.2` and pin it explicitly in `configured_session`/`fixture_request` —
  they prove wire parity, not the default model.
- PRD `provider-capability-gating` FR-2: `factory.rs` advertises the full
  session-control surface as superpower descriptors — `zai:plan` (alias
  `plan`: coding/standard/bigmodel), `zai:generation` (alias `generation`:
  balanced/precise/exploratory), and `zai:auxiliary` (alias `auxiliary`:
  `main` + every catalog model). The harness consumes these for its
  `/plan` / `/generation` / `/auxiliary` rows and value palettes.
  `policy.rs` (`GlmSuperpowerPolicy`) owns the choice rules: `auxiliary`
  keeps `main` plus plan-available non-vision models, `mixture` narrows
  tool-capable candidates to non-vision plan-available advisers, and
  `validate` rejects ineligible auxiliary selections. GLM predicates never
  leak into the TUI.

## Verification

- Run `cargo test -p vesper-provider-glm --all-features`.
- Run `cargo xtask provider glm verify`.
- Run strict workspace Clippy and architecture checks.

## Child DOX Index

No children.
