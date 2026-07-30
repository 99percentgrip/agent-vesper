# ADR 0003: Bypass Semantics

Status: APPROVED BY EXISTING REQUIREMENT

## Context

The source evaluates policy before Bypass; denial is absolute
(`agent.py:4805-4864`).

## Decision

Initial parity preserves the ordering exactly: Bypass removes interactive
approval only after top-level and nested policy evaluation. A denial remains
terminal.

## Consequences

Strengthening Bypass later is permitted only through an approved intentional
change and updated fixtures. Weakening denial is prohibited.

