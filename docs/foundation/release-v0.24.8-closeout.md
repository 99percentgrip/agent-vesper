# Release v0.24.8 closeout

## Objective and status

Release v0.24.8 completed.

- Commit: 9fada0cd06714ba1e754b332b5d44b39d3d9f373
- Local gates: 8/8 passed; current exact-SHA CI: 12 jobs verified.
- Published inventory: 16 assets; required archive/checksum verification passed.
- Current main: 9fada0cd06714ba1e754b332b5d44b39d3d9f373 (green).
- Registry: https://github.com/agentclientprotocol/registry/pull/539
- Execution report: /home/Alex/.local/state/agent-vesper/release-recovery/worktrees/abde39a508845675/20261007T134243820Z-b7cda1d78beb/docs/foundation/release-v0.24.8-closeout.md

Publication and registry/report delivery are complete. Full PRD coverage is tracked separately.

## Methods and commands

Native RRC reobserved current main and the complete exact-SHA workflow matrix. Registry delivery used the existing PR branch, an expected-blob contents update and independent readback; no PR was created, replaced or merged. Reports and links remain local.

## Files and exact evidence

```json
{
  "changed_repair_files": [],
  "changed_version_files": [
    "Cargo.toml",
    "apps/agent-vesper-acp/Cargo.toml",
    "apps/agent-vesper-tui/Cargo.toml",
    "crates/vesper-acp/Cargo.toml",
    "crates/vesper-agent/Cargo.toml",
    "crates/vesper-auth/Cargo.toml",
    "crates/vesper-bridge/Cargo.toml",
    "crates/vesper-checkpoints/Cargo.toml",
    "crates/vesper-cognition/Cargo.toml",
    "crates/vesper-config/Cargo.toml",
    "crates/vesper-harness/Cargo.toml",
    "crates/vesper-mcp/Cargo.toml",
    "crates/vesper-memory/Cargo.toml",
    "crates/vesper-policy/Cargo.toml",
    "crates/vesper-provider/Cargo.toml",
    "crates/vesper-provider-glm/Cargo.toml",
    "crates/vesper-provider-openai/Cargo.toml",
    "crates/vesper-provider-synthetic/Cargo.toml",
    "crates/vesper-provider-xai/Cargo.toml",
    "crates/vesper-runtime/Cargo.toml",
    "crates/vesper-sandbox/Cargo.toml",
    "crates/vesper-sessions/Cargo.toml",
    "crates/vesper-swarm/Cargo.toml",
    "crates/vesper-testkit/Cargo.toml",
    "crates/vesper-voice/Cargo.toml",
    "crates/vesper-voice-kokoro/Cargo.toml",
    "crates/vesper-web-fetch/Cargo.toml",
    "xtask/Cargo.toml",
    "registry/agent.json",
    "Cargo.lock"
  ],
  "current_main": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
  "epoch": "20261007T134347.377334367Z-b7cda1d78beb",
  "failures": [],
  "local_gates": [
    {
      "command": "cargo check --workspace --all-targets",
      "evidence_ref": "local:version-preparation",
      "name": "version-preparation",
      "state": "succeeded"
    },
    {
      "command": "cargo xtask verify",
      "evidence_ref": "local:workspace-verify:05be11e0e97e5ece6e0b7adb8fb769814259f8d3735a65cdb8192d792fd81943",
      "name": "workspace-verify",
      "state": "succeeded"
    },
    {
      "command": "cargo xtask acceptance",
      "evidence_ref": "local:acceptance:636e0452e16201d58418acfe5fe2cd144d8116362c17bfe2173a9fb68ab59af0",
      "name": "acceptance",
      "state": "succeeded"
    },
    {
      "command": "cargo xtask architecture",
      "evidence_ref": "local:architecture:c0e0aebe9b7a8443b26cdb032c4570c955b9dc2cbb395ecb23471f7b441fe176",
      "name": "architecture",
      "state": "succeeded"
    },
    {
      "command": "cargo xtask msrv",
      "evidence_ref": "local:msrv:c955b4fbfb5e310a6f847f95dd6d4dbea254f954049003e6a9c4c44bbcae3cd7",
      "name": "msrv",
      "state": "succeeded"
    },
    {
      "command": "cargo deny --all-features check",
      "evidence_ref": "local:supply-chain-policy:f1a0fca39d4280363937aabd77783990ea6480bd9ca257816de3b68fc8efa845",
      "name": "supply-chain-policy",
      "state": "succeeded"
    },
    {
      "command": "cargo audit",
      "evidence_ref": "local:advisories:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "name": "advisories",
      "state": "succeeded"
    },
    {
      "command": "cargo build --locked --release --package agent-vesper-acp --package agent-vesper-tui --package vesper-web-fetch --package vesper-sandbox --features agent-vesper-acp/docker,agent-vesper-tui/docker,agent-vesper-acp/swarm,agent-vesper-tui/swarm,agent-vesper-acp/bridge,agent-vesper-tui/bridge,agent-vesper-tui/voice-kokoro,agent-vesper-tui/voice-flm",
      "evidence_ref": "local:release-build:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "name": "release-build",
      "state": "succeeded"
    }
  ],
  "main_gates": [
    {
      "head_sha": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861472886,
          "job_name": "quality",
          "platform": "ubuntu-24.04",
          "run_id": 37641534734,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534734/job/112861472886",
          "workflow_id": 324058982,
          "workflow_name": "pull-request-validation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473461,
          "job_name": "supply-chain",
          "platform": "ubuntu-24.04",
          "run_id": 37641534734,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534734/job/112861473461",
          "workflow_id": 324058982,
          "workflow_name": "pull-request-validation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473618,
          "job_name": "windows-installer",
          "platform": "windows-2025",
          "run_id": 37641534734,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534734/job/112861473618",
          "workflow_id": 324058982,
          "workflow_name": "pull-request-validation"
        }
      ],
      "name": "pull-request-validation",
      "run_attempt": 1,
      "run_id": 37641534734,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534734"
    },
    {
      "head_sha": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473180,
          "job_name": "linux-x86_64",
          "platform": "ubuntu-24.04",
          "run_id": 37641534510,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534510/job/112861473180",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473357,
          "job_name": "macos-intel",
          "platform": "macos-15-intel",
          "run_id": 37641534510,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534510/job/112861473357",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473402,
          "job_name": "linux-arm64",
          "platform": "ubuntu-24.04-arm",
          "run_id": 37641534510,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534510/job/112861473402",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473426,
          "job_name": "windows-x86_64",
          "platform": "windows-2025",
          "run_id": 37641534510,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534510/job/112861473426",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861473443,
          "job_name": "macos-apple-silicon",
          "platform": "macos-15",
          "run_id": 37641534510,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534510/job/112861473443",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        }
      ],
      "name": "five-target-foundation",
      "run_attempt": 1,
      "run_id": 37641534510,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534510"
    },
    {
      "head_sha": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861471833,
          "job_name": "rust-1-88",
          "platform": "ubuntu-24.04",
          "run_id": 37641534461,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534461/job/112861471833",
          "workflow_id": 324058985,
          "workflow_name": "msrv"
        }
      ],
      "name": "msrv",
      "run_attempt": 1,
      "run_id": 37641534461,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534461"
    },
    {
      "head_sha": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861478636,
          "job_name": "Native namespace Hive",
          "platform": "ubuntu-22.04",
          "run_id": 37641534454,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534454/job/112861478636",
          "workflow_id": 351737017,
          "workflow_name": "web-driver"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861479526,
          "job_name": "Contained browser linux-x86_64",
          "platform": "ubuntu-24.04",
          "run_id": 37641534454,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534454/job/112861479526",
          "workflow_id": 351737017,
          "workflow_name": "web-driver"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112861479579,
          "job_name": "Contained browser linux-aarch64",
          "platform": "ubuntu-24.04-arm",
          "run_id": 37641534454,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534454/job/112861479579",
          "workflow_id": 351737017,
          "workflow_name": "web-driver"
        }
      ],
      "name": "web-driver",
      "run_attempt": 1,
      "run_id": 37641534454,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37641534454"
    }
  ],
  "metrics": {
    "ci_wait_millis": 3860000,
    "model_active_millis": 0,
    "repeated_fingerprints_blocked": 0,
    "retries_rejected": 0,
    "speculative_reruns_prevented": 0
  },
  "publication_verified": true,
  "published_assets": [
    "agent-vesper-acp-darwin-aarch64.tar.gz",
    "agent-vesper-acp-darwin-aarch64.tar.gz.sha256",
    "agent-vesper-acp-darwin-x86_64.tar.gz",
    "agent-vesper-acp-darwin-x86_64.tar.gz.sha256",
    "agent-vesper-acp-linux-aarch64.tar.gz",
    "agent-vesper-acp-linux-aarch64.tar.gz.sha256",
    "agent-vesper-acp-linux-x86_64.tar.gz",
    "agent-vesper-acp-linux-x86_64.tar.gz.sha256",
    "agent-vesper-acp-windows-x86_64.zip",
    "agent-vesper-acp-windows-x86_64.zip.sha256",
    "vesper-web-driver-linux-aarch64.image-id",
    "vesper-web-driver-linux-aarch64.tar.gz",
    "vesper-web-driver-linux-aarch64.tar.gz.sha256",
    "vesper-web-driver-linux-x86_64.image-id",
    "vesper-web-driver-linux-x86_64.tar.gz",
    "vesper-web-driver-linux-x86_64.tar.gz.sha256"
  ],
  "receipt": {
    "registry_blob": "897252a97046551a8f904a75199176317aa1b28a",
    "registry_url": "https://github.com/agentclientprotocol/registry/pull/539",
    "report": "/home/Alex/.local/state/agent-vesper/release-recovery/worktrees/abde39a508845675/20261007T134243820Z-b7cda1d78beb/docs/foundation/release-v0.24.8-closeout.md"
  },
  "release_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
  "repair_admissions": {},
  "repair_attempts": [],
  "repository": "/home/Alex/Projects/agent-vesper/.git",
  "retry_budget": {
    "full_gate_limit": 1,
    "full_gate_used": 0,
    "infrastructure_limit": 1,
    "infrastructure_used": 0,
    "repair_attempt_limit_per_family": 2,
    "targeted_diagnostic_limit": 2,
    "targeted_diagnostic_used": 0
  },
  "state_changes": [],
  "transitions": [
    {
      "at": "2026-10-07T13:43:47.377387897Z",
      "evidence_refs": [],
      "from": "idle",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": null,
      "reason": "release intent accepted by RRC",
      "repair_attempts": 0,
      "run_id": null,
      "sequence": 1,
      "source_commit": "b7cda1d78bebec842347731602a7b071a476b4b6",
      "to": "preparing",
      "workflow_id": null
    },
    {
      "at": "2026-10-07T13:43:47.377960442Z",
      "evidence_refs": [],
      "from": "preparing",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": null,
      "reason": "local release verification required before candidate creation",
      "repair_attempts": 0,
      "run_id": null,
      "sequence": 2,
      "source_commit": "b7cda1d78bebec842347731602a7b071a476b4b6",
      "to": "local_verification",
      "workflow_id": null
    },
    {
      "at": "2026-10-07T15:00:58.386655243Z",
      "evidence_refs": [
        "local:version-preparation",
        "local:workspace-verify:05be11e0e97e5ece6e0b7adb8fb769814259f8d3735a65cdb8192d792fd81943",
        "local:acceptance:636e0452e16201d58418acfe5fe2cd144d8116362c17bfe2173a9fb68ab59af0",
        "local:architecture:c0e0aebe9b7a8443b26cdb032c4570c955b9dc2cbb395ecb23471f7b441fe176",
        "local:msrv:c955b4fbfb5e310a6f847f95dd6d4dbea254f954049003e6a9c4c44bbcae3cd7",
        "local:supply-chain-policy:f1a0fca39d4280363937aabd77783990ea6480bd9ca257816de3b68fc8efa845",
        "local:advisories:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "local:release-build:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
      ],
      "from": "local_verification",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": null,
      "reason": "local release verification passed",
      "repair_attempts": 0,
      "run_id": null,
      "sequence": 3,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "candidate_ready",
      "workflow_id": null
    },
    {
      "at": "2026-10-07T15:01:03.953334333Z",
      "evidence_refs": [
        "origin/main@9fada0cd06714ba1e754b332b5d44b39d3d9f373"
      ],
      "from": "candidate_ready",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": null,
      "reason": "exact-commit remote gates dispatched",
      "repair_attempts": 0,
      "run_id": null,
      "sequence": 4,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "remote_gate_running",
      "workflow_id": null
    },
    {
      "at": "2026-10-07T15:01:04.787047168Z",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "from": "remote_gate_running",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": null,
      "reason": "required workflow runs have not appeared for the exact candidate SHA",
      "repair_attempts": 0,
      "run_id": null,
      "sequence": 5,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "waiting_for_matrix",
      "workflow_id": null
    },
    {
      "at": "2026-10-07T15:58:28.770359382Z",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "from": "waiting_for_matrix",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": 112861472886,
      "reason": "all exact-commit required gates are terminal and green",
      "repair_attempts": 0,
      "run_id": 37641534734,
      "sequence": 6,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "remote_gates_green",
      "workflow_id": 324058982
    },
    {
      "at": "2026-10-07T15:58:32.231407341Z",
      "evidence_refs": [
        "git:tag:a4e8975221019611a6c44e3246e9d9ef94af4980"
      ],
      "from": "remote_gates_green",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": 112861472886,
      "reason": "annotated immutable tag verified at exact green commit",
      "repair_attempts": 0,
      "run_id": 37641534734,
      "sequence": 7,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "tagging",
      "workflow_id": 324058982
    },
    {
      "at": "2026-10-07T15:58:32.231857196Z",
      "evidence_refs": [
        "git:tag:v0.24.8"
      ],
      "from": "tagging",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": 112861472886,
      "reason": "release publication started after exact-commit gates",
      "repair_attempts": 0,
      "run_id": 37641534734,
      "sequence": 8,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "publishing",
      "workflow_id": 324058982
    },
    {
      "at": "2026-10-07T16:22:07.865121542Z",
      "evidence_refs": [
        "github:run:37648330813",
        "github:release:v0.24.8"
      ],
      "from": "publishing",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": 112861472886,
      "reason": "release assets and metadata published and verified",
      "repair_attempts": 0,
      "run_id": 37641534734,
      "sequence": 9,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "published",
      "workflow_id": 324058982
    },
    {
      "at": "2026-10-07T16:22:12.388058349Z",
      "evidence_refs": [
        "rrc:verified-publication-closeout"
      ],
      "from": "published",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": 112861472886,
      "reason": "post-release main-health closeout started",
      "repair_attempts": 0,
      "run_id": 37641534734,
      "sequence": 10,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "post_release_closeout",
      "workflow_id": 324058982
    },
    {
      "at": "2026-10-07T16:22:13.289254892Z",
      "evidence_refs": [],
      "from": "post_release_closeout",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": null,
      "reason": "post-release closeout refreshes current main gates",
      "repair_attempts": 0,
      "run_id": null,
      "sequence": 11,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "waiting_for_matrix",
      "workflow_id": null
    },
    {
      "at": "2026-10-07T16:22:41.601066647Z",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "from": "waiting_for_matrix",
      "full_gate_retries": 0,
      "infrastructure_retries": 0,
      "job_id": 112861472886,
      "reason": "post-release main is green; published release remains unchanged",
      "repair_attempts": 0,
      "run_id": 37641534734,
      "sequence": 12,
      "source_commit": "9fada0cd06714ba1e754b332b5d44b39d3d9f373",
      "to": "complete",
      "workflow_id": 324058982
    }
  ],
  "version": "0.24.8"
}
```

## Deviations, unresolved items and readiness effect

Upstream PR merge remains maintainer-owned. Foundation fixtures do not establish live provider effectiveness. This report certifies the observed release and closeout, not complete implementation/PRD parity. The published tag and assets remain immutable.
