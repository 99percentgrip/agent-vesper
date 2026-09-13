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
- `icon.svg` — required 16×16 monochrome Vesper icon, mirrored alongside the
  manifest in the existing upstream registry PR. Use `currentColor`/`none`
  and omit XML comments for registry documentation compatibility.

## Local Contracts

- `agent.json.version` MUST equal `Cargo.toml` `[workspace.package].version`
  at every release. Bumping one without the other is a release defect.
- `distribution.binary` archive URLs MUST point at GitHub release artifacts
  produced by CI for the matching tag (`v<version>`).
- The published binary `cmd` is `agent-vesper-acp` (or `.exe` on Windows)
  inside the archive's `agent-vesper-acp/` bundle directory, matching the
  installer's expectations in `scripts/install.sh` / `scripts/install.ps1`.
- `license_url` links to the repository license text, as required by the upstream
  registry schema; retain the SPDX `license` identifier as well.
- The manifest is data only; no executable ships here.

## Verification

- `jq . registry/agent.json` parses (valid JSON).
- `agent.json.version` matches `Cargo.toml` workspace version.
- The `id` is stable across releases (`agent-vesper`).
- Validate the manifest and icon with the current upstream registry schema and
  entry validator; published archive URLs must resolve after release.

## Child DOX Index

No children.
