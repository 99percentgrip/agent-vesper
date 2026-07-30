# ADR 0002: Reasoning Persistence Compatibility

Status: APPROVED BY EXISTING REQUIREMENT

## Context

Python persists `reasoning_content` by default and removes it only when
`GLM_ACP_PERSIST_REASONING=0` (`config.py:669-682`, `agent.py:555-562`).

## Decision

Parity preserves this behavior and imports either form. Provider-specific
opaque reasoning blocks remain namespaced and subject to one explicit privacy
policy. No default is silently changed.

## Consequences

A safer new-store default may be proposed later, but requires product approval
and must not alter imported legacy data.

