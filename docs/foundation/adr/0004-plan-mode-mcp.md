# ADR 0004: Plan Mode MCP Compatibility

Status: APPROVED BY EXISTING REQUIREMENT

## Context

The source permits generic MCP list/call in Plan Mode while restricting other
destructive operations (`agent.py:4845-4860`).

## Decision

Capture and preserve the current allowance for initial parity.

## Consequences

The allowance is an intentional review point, not a recommended permanent
security posture. Restriction requires product approval and a versioned policy
change.

