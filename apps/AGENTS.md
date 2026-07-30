# Composition applications

## Purpose

Own thin deployable binaries that compose production crates without absorbing
domain, provider, runtime, or protocol business logic.

## Local Contracts

- Applications may wire concrete adapters at composition boundaries.
- Stdout contracts of protocol binaries are inviolable.
- Startup configuration must be secret-safe and default to secure endpoints.
- Applications may inject bounded read-only session roots but must not create
  those roots or expose persistence mutation.

## Child DOX Index

- `agent-vesper-acp/AGENTS.md` — ACP stdio composition and process lifecycle.
