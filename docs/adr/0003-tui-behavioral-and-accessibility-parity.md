# ADR 0003: TUI Behavioral and Accessibility Parity

Status: ACCEPTED

## Context

The Python TUI has extensive workflows and accessibility behavior. Reproducing
its Textual widget tree would constrain a native Rust frontend without improving
observable compatibility.

## Decision

The future Rust TUI requires behavioral and accessibility parity, not pixel or
widget identity. Session workflows, prompt queue/cancellation, commands and
palette actions, settings, permissions, plans, tools, permitted reasoning,
usage/activity, keybindings, Vim mode, screen-reader operation, terminal
restoration, clipboard, images, local voice, notifications, mobile approval,
worktrees, and export remain in scope. Every important action must remain
available without a mouse.

No workflow or accessibility operation may be removed without a later ADR.

## Alternatives considered

- Pixel identity: rejected as brittle and not behaviorally meaningful.
- Reduced “minimum” TUI: rejected because it silently removes proven workflows.
- Direct provider-to-TUI coupling: rejected because it breaks frontend isolation.

## Consequences

The TUI will consume harness events and commands through frontend-neutral ports.
Ratatui-specific state cannot enter foundational domain contracts.

## Compatibility implications

Visual layout may change while commands, state transitions, recovery, and
accessible operation remain equivalent.

## Security implications

Permission UX cannot create authority, and reasoning display must honor retention
and redaction. Terminal restoration is a failure-path requirement.

## Migration implications

TUI work starts only at its owning stage after reducers and event contracts exist.

## Verification requirements

Command/action inventories, reducer fixtures, terminal restoration tests, and
screen-reader audits are release gates.

## Evidence

- Historical decision: [foundation ADR 0006](../foundation/adr/0006-tui-compatibility.md)
- Inventory: [module map](../recon/python-to-rust-module-map.md)
- Behavioral scope: [behavioral contract](../recon/behavioral-contract.md)
