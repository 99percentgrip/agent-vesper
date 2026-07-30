# Minimal runtime report

Status: COMPLETE

## Objective and result

`vesper-runtime` implements the smallest provider-neutral execution boundary
needed for ACP no-tools turns. It owns a bounded process event stream, an
in-memory session registry, and one serialized actor per session. It performs no
filesystem or database I/O.

## Ownership

- `supervisor.rs`: command routing, registry, lifecycle, shutdown, and bounded
  event delivery.
- `registry.rs`: type-erased neutral `ProviderFactory` registration with
  duplicate/unknown rejection.
- `session.rs`: immutable snapshots and turn results.
- `cancellation.rs`: runtime → session → turn/provider cancellation tokens.
- `error.rs`: safe typed runtime failures.

Each actor has a 16-command mailbox. The shared event receiver has capacity 64.
Prompt work is owned and cancel-selectable; no global session-state mutex or
detached task exists. One session serializes prompts while independent actors
allow other sessions to progress.

## Minimal turn

The actor appends accepted user content, constructs a neutral request with no
tools, translates ordered provider events, stores only assistant-visible
content, and updates cumulative usage. A provider tool call is surfaced,
reported unavailable, and terminated exactly once without execution or a
second provider request.

## Verification and limits

Runtime unit tests cover provider registration, ephemeral lifecycle/lineage,
ordered prompt events, and message linkage. Process tests additionally exercise
cancellation and clean shutdown. Persistent sessions, compaction, policy
orchestration, and the agent/tool loop are intentionally absent.

