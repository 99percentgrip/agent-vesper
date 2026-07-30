# ADR 0007: Session Actor and Hierarchical Cancellation

Status: ACCEPTED

## Context

The behavioral contract requires ordered session events, isolation between
sessions, cancellation during provider streams and tools, and cleanup of nested
work. Shared mutable global state or uncontrolled mutex graphs would make those
rules difficult to prove.

## Decision

Each future active session is owned by a single session actor that serializes
session state transitions and emits monotonic event sequences. Long work runs in
owned child tasks and returns results to the actor. Cancellation is hierarchical:
application → session → turn → provider/tool/worker. Terminal cancellation is a
domain outcome, not a generic error.

No callback may hold session state while awaiting inbound work that requires the
same actor. Global state is limited to immutable registries or narrowly owned
services.

## Alternatives considered

- Global `Arc<Mutex<...>>`: rejected because ownership and lock ordering become
  implicit.
- Provider-owned sessions: rejected because policy and lifecycle authority leak.
- Independent cancellation flags: rejected because descendants can outlive turns.

## Consequences

Message passing and explicit ownership replace lock re-entry. Event sequence IDs
are domain types. Runtime selection remains an implementation-stage decision.

## Compatibility implications

ACP ordering, prompt cancellation, session lineage, and replay must remain
observable at parity even though internals change.

## Security implications

Cancellation must propagate to process groups and descendants. No post-cancel
event or output may escape an owned child task.

## Migration implications

Stage 1 defines IDs, event envelopes, and cancellation-facing provider contracts;
the actor itself belongs to the core stage.

## Verification requirements

Model-based actor tests, cancellation race tests, event-order fixtures, shutdown
tests, and process-tree conformance are mandatory before parity.

## Evidence

- [behavioral contract](../recon/behavioral-contract.md)
- [Rust architecture proposal](../recon/rust-architecture-proposal.md)
- [ACP SDK spike](../foundation/acp-rust-sdk-spike.md)
