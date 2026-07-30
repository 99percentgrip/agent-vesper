# ADR 0006: TUI Compatibility Level

Status: RECOMMENDED

## Context

The source TUI exposes commands, palette actions, configurable bindings,
screen-reader behavior, mouse/Vim modes, media, and session/tool/plan state
(`tui.py`, `tests/test_tui.py`).

## Decision

Require behavioral and accessible parity for every operation and state
transition. Exact pixels, widget hierarchy, and layout are not contractual
when equivalent keyboard, mouse, screen-reader, and export behavior exists.

## Consequences

Command/binding/accessibility catalogs become fixtures. Any retired operation
or materially changed workflow requires user approval.

