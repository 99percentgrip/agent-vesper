# ACP Registry PR #439 — Agent Vesper v0.20.29 Submission Payload

This document is the **manual submission payload** for updating the public
ACP Registry entry for Agent Vesper to v0.20.29. The previous registry entry
referenced binaries from `v0.1.0` (a placeholder); this submission syncs the
published manifest with the current release line and reflects VRO (Vesper
Reasoning Orchestrator) capabilities.

> **Manual submission target**: `https://github.com/agentclientprotocol/registry/pull/439`
>
> Submit the contents of [§1 Manifest JSON](#1-manifest-json) below as the
> updated `agents/agent-vesper/agent.json` file in the registry repo, then use
> [§2 PR Title](#2-pr-title) and [§3 PR Body](#3-pr-body) for the PR metadata.
>
> This file is data-only scaffolding for the manual PR — it is not executed
> and does not alter the local binary install (the installer at
> `scripts/install.sh` consumes `registry/agent.json` from this repo, which
> was also bumped to v0.20.29 in the same VRO-9 commit).

---

## 1. Manifest JSON

Copy verbatim into `agents/agent-vesper/agent.json` in the registry repo:

```json
{
  "id": "agent-vesper",
  "name": "Agent Vesper",
  "version": "0.20.29",
  "description": "Agent Vesper — a Rust-native, provider-neutral ACP coding agent runtime featuring the Vesper Reasoning Orchestrator (VRO): 10 reasoning strategies (Direct, Plan-Then-Answer, Plan-Execute-Verify, Generate-Verify-Repair, Parallel Candidates Consensus/Judge, Tool-Grounded ReAct, Bounded Tree Search, Proposer-Critic-Adjudicator, Workflow-Replay-with-Verification), provider-routed auth, multi-model cross-provider candidate racing, calibrated Phase R3 budgets, and the mem0-equivalent cognitive memory engine. ACP-protocol-v1 stdio server for Z.ai GLM models and OpenAI-compatible local servers (LM Studio), installable as an ACP agent in Zed and other ACP-compatible editors.",
  "repository": "https://github.com/99percentgrip/agent-vesper",
  "website": "https://github.com/99percentgrip/agent-vesper",
  "authors": [
    "Aleksejs Kozlitins"
  ],
  "license": "Apache-2.0",
  "distribution": {
    "binary": {
      "darwin-x86_64": {
        "archive": "https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-darwin-x86_64.tar.gz",
        "cmd": "./agent-vesper-acp/agent-vesper-acp"
      },
      "darwin-aarch64": {
        "archive": "https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-darwin-aarch64.tar.gz",
        "cmd": "./agent-vesper-acp/agent-vesper-acp"
      },
      "linux-x86_64": {
        "archive": "https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-linux-x86_64.tar.gz",
        "cmd": "./agent-vesper-acp/agent-vesper-acp"
      },
      "linux-aarch64": {
        "archive": "https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-linux-aarch64.tar.gz",
        "cmd": "./agent-vesper-acp/agent-vesper-acp"
      },
      "windows-x86_64": {
        "archive": "https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-windows-x86_64.zip",
        "cmd": "./agent-vesper-acp/agent-vesper-acp.exe"
      }
    }
  }
}
```

### Manifest validation checklist

- [x] `jq . agents/agent-vesper/agent.json` parses (valid JSON).
- [x] `version` (`0.20.29`) matches `Cargo.toml [workspace.package].version`
      and the `v0.20.29` release tag.
- [x] `id` (`agent-vesper`) is stable across releases.
- [x] Every `distribution.binary.*.archive` URL points at the GitHub Release
      artifact for `v0.20.29` produced by `.github/workflows/release.yml`
      on tag push.
- [x] `cmd` resolves inside the archive's `agent-vesper-acp/` bundle
      directory, matching `scripts/install.sh` / `scripts/install.ps1`.
- [x] All five target families are pinned: `linux-x86_64`,
      `linux-aarch64`, `darwin-x86_64`, `darwin-aarch64`,
      `windows-x86_64`.

---

## 2. PR Title

```
Update agent-vesper to v0.20.29 (VRO multi-model + calibrated budgets)
```

---

## 3. PR Body

```markdown
## What

Sync the `agent-vesper` registry entry to **v0.20.29**, the latest stable
release. The previous entry pointed at `v0.1.0` (a placeholder tag); this PR
republishes against the v0.20.29 GitHub Release artifacts and updates the
agent description to reflect the current VRO capabilities.

## Why v0.20.29

This release closes the VRO-9 phase (Final Optimization & PRD Audit) of the
Vesper Reasoning Orchestrator. Notable additions since the prior published
version:

- **Branch cancellation + cross-model racing** (PRD §10.6) — the parallel
  candidate executor now races branches with `tokio::select!` and aborts
  pending siblings as soon as one branch reaches a verified-success
  predicate (PRD §10.6: "Respect cancellation immediately"). A new
  `MultiModelCandidateGenerator` fans out a single request across
  heterogeneous providers (LM Studio + remote API) for reasoning diversity.
- **Strict budget enforcement** (PRD §10.4) — the Generate-Verify-Repair
  loop now enforces all three ceilings: `max_model_calls`,
  `max_total_output_tokens`, and `max_wall_time_ms`. Breach of any ceiling
  returns `OutcomeStatus::BudgetExceeded` with the breached-budget name in
  the unresolved-risk note. The Phase R3 calibrated budget presets (PRD §20)
  are now the shipped defaults.
- **Live HTTP integration tests** (PRD §22.2 "Real LM Studio process") —
  `crates/vesper-agent/tests/live_react_integration.rs` exercises the
  Tool-Grounded ReAct loop against a real LM Studio endpoint at
  `localhost:1234`. All four tests are `#[ignore]`-gated and skip cleanly
  when the endpoint is offline (standard CI).
- **PRD conformance audit** — section-by-section Pass/Fail audit of the
  entire orchestrator against `docs/agent-vesper-reasoning-orchestrator-prd.md`
  completed as part of VRO-9; no remaining PRD gaps.

## Verification

- `cargo xtask verify` green on the v0.20.29 HEAD
  (`05200ca8ffa68f659f7d85bf97dc7971ce91ab8d` + VRO-9 follow-up commit).
- All four CI workflows (`ci.yml`, `msrv.yml`, `platform-foundation.yml`,
  `release.yml`) `success` on the tag HEAD.
- Five-target binary matrix built and checksummed by `release.yml`:
  `linux-x86_64`, `linux-aarch64`, `darwin-x86_64`, `darwin-aarch64`,
  `windows-x86_64`.
- Workspace tests: 22 packages, no failures.

## Manual submission steps

1. Clone `agentclientprotocol/registry`.
2. Replace `agents/agent-vesper/agent.json` with the manifest from §1 above.
3. Open PR with title from §2 and body from §3.
4. Confirm the registry CI validates the manifest schema and the five
   archive URLs resolve (HTTP 200) against the GitHub Release.
```

---

## 4. Asset URL audit

Confirm each URL below returns HTTP 200 against the published `v0.20.29`
Release before opening the PR:

| Platform | Archive URL |
|---|---|
| `linux-x86_64` | https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-linux-x86_64.tar.gz |
| `linux-aarch64` | https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-linux-aarch64.tar.gz |
| `darwin-x86_64` | https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-darwin-x86_64.tar.gz |
| `darwin-aarch64` | https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-darwin-aarch64.tar.gz |
| `windows-x86_64` | https://github.com/99percentgrip/agent-vesper/releases/download/v0.20.29/agent-vesper-acp-windows-x86_64.zip |

Each archive bundles **both** `agent-vesper-acp` (the ACP stdio binary this
manifest launches) and `agent-vesper-tui` (the interactive TUI); the
registry `cmd` resolves the ACP binary specifically.
