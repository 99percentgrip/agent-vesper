# ACP registry manifest

## Purpose

Own the Agent Vesper entry published to the ACP Registry so Zed and other
ACP-compatible editors can discover and install the compiled
`agent-vesper-acp` binary exactly like the original Python `native-glm-acp`
distribution.

## Ownership

- `agent.json` — the canonical ACP registry manifest (`id`, `name`, `version`,
  per-platform binary archives under `distribution.binary`). Mirrors the
  Python oracle's `registry/agent.json` schema.
- `icon.svg` — optional agent icon (may be added later).

## Local Contracts

- `agent.json.version` MUST equal `Cargo.toml` `[workspace.package].version`
  at every release. Bumping one without the other is a release defect.
- `distribution.binary` archive URLs MUST point at GitHub release artifacts
  produced by CI for the matching tag (`v<version>`).
- The published binary `cmd` is `agent-vesper-acp` (or `.exe` on Windows)
  inside the archive's `agent-vesper-acp/` bundle directory, matching the
  installer's expectations in `scripts/install.sh` / `scripts/install.ps1`.
- The manifest is data only; no executable ships here.

## Verification

- `jq . registry/agent.json` parses (valid JSON).
- `agent.json.version` matches `Cargo.toml` workspace version.
- The `id` is stable across releases (`agent-vesper`).

## Child DOX Index

No children.
