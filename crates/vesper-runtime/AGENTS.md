# Minimal provider runtime

## Purpose

Own provider-neutral process supervision, session actors, command routing,
bounded event delivery, hierarchical cancellation, one-request no-tools
provider turns, and acceptance of pure converted session state.

## Local Contracts

- May depend on `vesper-sessions` read-only repository, converted-state, and
  transactional writer ports, but not implement filesystem access itself.
- Do not depend on ACP, GLM, frontends, persistence I/O, SQLite, or tool
  execution.
- Every session owns mutable state in one serialized actor.
- Runtime, session, turn, and provider tasks are cancellation descendants.
- Channels are bounded; visible events are never silently dropped.
- A provider tool call is surfaced and terminates as unsupported without
  execution or another provider request. **This contract is preserved under
  ADR 0010 (Tier C):** the runtime remains the pure, provider-neutral
  single-turn engine. Tool execution and the multi-turn agent loop live in a
  separate `vesper-agent` crate that *composes* this runtime — they do not
  enter the runtime itself.
- No filesystem writes or detached tasks; the optional `save_session` path
  flows entirely through the injected `RuntimeSessionWrites` port.
- A composed external multi-turn engine may commit a completed visible turn
  through `accept_external_turn`; the actor remains the sole owner of history
  and the existing injected writer remains the only persistence path.
- Freshly created sessions carry a default endpoint identity supplied by the
  composition boundary through `RuntimeDefaults.endpoint`, so the converted
  record is always persistable; the runtime is provider-neutral and never
  hardcodes an endpoint.
- Configuration-required restored sessions remain inspectable/replayable and
  reject provider dispatch until configuration is resolved.
- In-memory actors win ID collisions; persistent reads occur only on actor
  misses, and missing IDs create ephemeral sessions without storage writes.
- Close removes only the actor. Fork remains in-memory and never mutates the
  source record.
- Persistent adoption is serialized by session ID, rechecks actor ownership
  after I/O, and never lets stale disk state overwrite a newer actor. Separate
  IDs remain concurrent.
- `ProviderRegistry::register_with_superpowers` lets the composition boundary
  register a factory together with its [`ProviderSuperpowers`] surface;
  `superpowers(provider_id)` and `all_superpowers()` let a frontend discover
  the active provider's native controls at startup without taking a
  dependency on any concrete adapter crate.
- Sessions carry a **session-scoped reasoning override** (`SessionSnapshot.reasoning`,
  seeded from `RuntimeDefaults.reasoning`) mutated by the `UpdateSessionReasoning`
  command (ADR 0009). `drive_prompt` sources each turn's `ProviderRequest.reasoning`
  from the snapshot override, falling back to the runtime default when none is
  set. The mode label is opaque/provider-neutral at the command boundary.
- Sessions also carry a **session-scoped provider configuration overlay**
  mutated by the `UpdateProviderConfiguration` command: the caller sends a
  fully merged provider envelope (plus an optional replacement model); the
  actor merges the overlay onto the session's envelope, rejects any provider
  identity change, validates the merged configuration through a throwaway
  `ProviderRegistry::create_session` round-trip (the owning adapter is the
  only validator — the runtime never interprets provider keys), then bumps
  the session revision. Runtime-wide updates (`session_id == None`) remain
  unsupported. This is the path ACP footer selectors (model picker, API
  plan, generation profile) use to take effect on the next turn.

## Verification

- Run `cargo test -p vesper-runtime --all-features`.
- Run `cargo xtask runtime verify` and architecture checks.

## Child DOX Index

No children.
