# Runtime Persistent Load and Resume

Status: COMPLETE

## Boundary

`vesper-runtime::RuntimeSessionReads` injects a `SessionRepository` plus legacy
and Agent Vesper decoders/converters. Runtime remains free of filesystem
implementation and concrete-provider/ACP dependencies.

## Lifecycle

For list, runtime merges:

1. active in-memory actors;
2. Agent Vesper read-store metadata;
3. legacy Native GLM ACP metadata.

Duplicate IDs resolve to the first source in that order. Results retain their
read-only origin and deterministic metadata ordering.

For load/resume:

1. return the existing in-memory actor when present;
2. acquire a per-session keyed load gate;
3. recheck the actor registry;
4. query configured readers in fixed precedence;
5. decode and convert the selected record;
6. recheck before actor insertion so a stale disk read cannot replace newer
   in-memory state;
7. create one actor and deliver replay;
8. complete the lifecycle response only after replay acceptance.

Concurrent loads for one ID serialize through the keyed gate. Separate IDs may
read concurrently, bounded by the filesystem semaphore. There is no global
filesystem lock and no global session-state mutex.

## Missing and invalid records

- Missing IDs use the source-compatible ephemeral-new-session behavior without
  writing a record.
- Corrupt, unsupported, over-bound, permission-denied, and unsafe-path records
  return typed runtime errors.
- ACP maps those errors to bounded safe classifications without terminating
  the dispatcher or exposing full private paths.
- Unknown provider/model/endpoint records remain inspectable and replayable,
  but new provider turns return configuration-required.

## Fork and close

Fork creates a new in-memory actor with copied safe history/configuration and
explicit parent/branch lineage. Close removes/cancels only the actor. Neither
operation mutates its persistent source.

## Composition

`agent-vesper-acp` enables readers only through explicit configuration:

- `AGENT_VESPER_ENABLE_SESSION_READS`
- `AGENT_VESPER_ENABLE_LEGACY_SESSION_READS`
- injected Agent Vesper and legacy roots
- bounded maximum record bytes
- bounded maximum listing entries

Startup validates roots and never creates them. Tests isolate all environment
and use synthetic stores.

## Process evidence

The real binary path
`agent-vesper-acp → vesper-acp → vesper-runtime → vesper-sessions` exercises
listing, loading, resuming, fallback, replay filtering, fork, close, invalid
records, collision precedence, and concurrent same-ID loads. The Stage 5 disk
invariance proof shows exact before/after file-set, hash, size, and timestamp
equality for every vector.

