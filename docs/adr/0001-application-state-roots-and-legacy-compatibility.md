# ADR 0001: Application State Roots and Legacy Compatibility

Status: ACCEPTED

## Context

Native GLM ACP owns legacy state including `~/.glm-acp` and project-local
`.glm-acp` content. Agent Vesper needs an independent identity without risking
silent movement, overwrite, or deletion of proven user data.

## Decision

Agent Vesper separates configuration, data, cache, and state. Linux uses the
corresponding XDG roots with an `agent-vesper/` suffix. macOS uses Application
Support, Caches, and Logs conventions. Windows uses roaming application data for
roaming-safe configuration and local application data for large or machine-local
data. Legacy locations are discoverable through read-only descriptors only.

Any later importer must be explicit and support dry-run output, manifests,
hashes, backups, rollback, and coexistence. Stage 1 neither reads private user
state nor writes any resolved application path.

## Alternatives considered

- Reusing `.glm-acp`: rejected because it couples identities and risks overwrite.
- Silent one-time migration: rejected because it is neither reversible nor
  safely auditable.
- Ignoring project-local legacy data: rejected because it would lose compatibility.

## Consequences

Path categories remain distinct in configuration contracts. A future migration
service owns writes; foundational path resolution grants no filesystem authority.

## Compatibility implications

Legacy state stays in place and initially remains read-only. `.agent-vesper` may
be introduced later, but no automatic project-local rename is authorized.

## Security implications

Import cannot become an implicit write capability. Paths are typed, traversal is
rejected, and migration must validate containment, hashes, permissions, and
symlinks.

## Migration implications

Session, memory, skill, and bundle migrations must consume the compatibility
descriptors and add explicit product UX before performing writes.

## Verification requirements

Injected-platform tests cover Linux, macOS, and Windows strategies; tests must
verify category separation, traversal rejection, legacy read-only status, and
absence of writes.

## Evidence

- Historical decision: [foundation ADR 0001](../foundation/adr/0001-state-location.md)
- Compatibility inventory: [persistence report](../recon/persistence-and-compatibility.md)
- Stage 1 contract: `crates/vesper-config/src/paths.rs`
