---
name: add-glm-model-release
description: Add a newly shipped Z.ai GLM model to the legacy Python glm-acp registry-driven pickers and ship its full release (config edits, hardcoding test consumers, docs, 5-file version bump, tag, binaries, reinstall, Registry PR). Use when the legacy agent must expose a new GLM model.
version: 1.1.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [model-registry, release, glm, registry-pr, legacy]
prerequisites:
  commands: [uv, gh]
---

# Add GLM Model + Release (legacy Python agent)

The whole feature is registry-driven — edit `glm_acp/config.py` only:

1. `MODELS`: new entry (id, name, description, context_window, plans).
   Rename the previous flagship from "(Flagship)" to its bare name.
2. `CONTEXT_WINDOW_TOKENS[new_id]`.
3. `DEFAULT_MODEL = new_id`.
4. `THOUGHT_LEVELS["high"]["models"]` and `["max"]["models"]`: append the
   new id next to the old flagship (deep `reasoning_effort: high|max` gates
   on both).
5. `API_ENDPOINTS["coding"]["description"]`: prepend the new model name.
6. `VISION_MODELS` only if the model is multimodal.

Pickers, auxiliary-model options, Mixture-of-Agents advisers, system-prompt
identity, and endpoint fallback all follow the registry — no other source
wiring.

## Test consumers that hardcode expectations (always need updates)

- `tests/test_config.py`: context-window asserts, plan membership counts,
  thought levels for the new model.
- `tests/test_agent.py`: auxiliary option values, model-option counts,
  endpoint-fallback default, MoA untouched-model default, `agent_info.version`.
- `tests/test_tui.py`: settings-screen model value set.
- `tests/test_reliability.py`: MoA adviser pair = the FIRST TWO non-primary
  models of the plan registry order.
- `tests/test_quality.py`: `benchmarks/run_live.py` echoes `DEFAULT_MODEL`
  dynamically.

## Docs

- `README.md`: feature bullet, config-options table, models table, installer
  version pins.
- Root `AGENTS.md`: models list, new vX.Y.Z status entry, dist-info expect
  line.
- `glm_acp/AGENTS.md`: deep-thinking section (both flagships) and the
  text-only models list.

## Version bump — all 5 files

`glm_acp/__init__.py`, the `tests/test_agent.py` version assertion,
`registry/agent.json` (version + all 6 archive URLs), `AGENTS.md`,
`README.md`.

## Verify

`uv run ruff check . && uv run pytest`.

## Release

1. Feature branch → squash-merge to `main` as `vX.Y.Z: Title` →
   `git tag vX.Y.Z` → push `main` + tag.
2. PITFALL: the squash-merge conflicts in every version-bumped file
   (identical-content double-commit). Resolve with
   `git checkout --theirs <changed files>`, `git add -u`, then confirm
   `git diff --cached <branch> --stat` is empty before committing.
3. `gh run watch <release-run-id> --exit-status` — then confirm the release
   has all assets (5 binaries + checksums, wheel, sdist, agent.json, icon,
   2 installers).
4. Reinstall local: `bash scripts/install.sh`; verify `--version` and that
   the new model id appears in `glm-acp chat --help` `--model` choices.

## ACP Registry PR

`gh pr edit` FAILS with a GraphQL Projects-classic deprecation error — use
REST: `gh api repos/agentclientprotocol/registry/pulls/<PR#> -X PATCH -f
title=... -f body=...`. Update the fork branch's `native-glm-acp/agent.json`
by copying the repo's `registry/agent.json` and pushing.

## Cross-repo

The Agent Vesper Rust repo mirrors this registry pattern — apply the same
model-addition there and audit its own registry-driven test consumers
before releasing.

## Provenance

Migrated from the legacy native-glm-acp (Python) learned-skill store.
