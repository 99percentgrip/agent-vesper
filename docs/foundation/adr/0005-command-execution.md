# ADR 0005: Command Execution Contract

Status: APPROVED BY EXISTING REQUIREMENT

## Context

Hooks are argv-only, but Python `run_command` accepts a shell string
(`hooks.py:41-77`, `tools.py:1975-2082`).

## Decision

Define separate argv-native and explicit-shell intents. Both are destructive,
policy/permission gated, cwd/root scoped, environment scrubbed, timeout
bounded, and owned by a process-tree supervisor. Platform quoting belongs only
to explicit shell adapters.

## Consequences

Legacy `run_command` scenarios retain shell semantics during parity. Removal
or deprecation of shell compatibility requires product approval.

