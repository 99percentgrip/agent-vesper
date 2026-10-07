# Release v0.24.7 closeout

## Objective and status

Release v0.24.7 completed.

- Commit: 0857f02b8dbdc2a867cdb93bf86a03559fb7555b
- Local gates: 8/8 passed; current exact-SHA CI: 11 jobs verified.
- Published inventory: 16 assets; required archive/checksum verification passed.
- Current main: 0857f02b8dbdc2a867cdb93bf86a03559fb7555b (green).
- Registry: https://github.com/agentclientprotocol/registry/pull/539
- Execution report: /tmp/vesper-rrc-closeout-fix/docs/foundation/release-v0.24.7-closeout.md

Publication and registry/report delivery are complete. Full PRD coverage is tracked separately.

## Methods and commands

Native RRC reobserved current main and the complete exact-SHA workflow matrix. Registry delivery used the existing PR branch, an expected-blob contents update and independent readback; no PR was created, replaced or merged. Reports and links remain local.

## Files and exact evidence

```json
{
  "repository": "/home/Alex/Projects/agent-vesper/.git",
  "epoch": "20261006T202106.463043330Z-0857f02b8dbd",
  "version": "0.24.7",
  "release_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
  "current_main": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
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
  "local_gates": [
    {
      "name": "version-preparation",
      "state": "succeeded",
      "command": "cargo check --workspace --all-targets",
      "evidence_ref": "local:version-preparation"
    },
    {
      "name": "workspace-verify",
      "state": "succeeded",
      "command": "cargo xtask verify",
      "evidence_ref": "local:workspace-verify:ddca515d4796381703dcbe24fe0da1b3a9eee891da977ec77cf02c49c8adcd0f"
    },
    {
      "name": "acceptance",
      "state": "succeeded",
      "command": "cargo xtask acceptance",
      "evidence_ref": "local:acceptance:636e0452e16201d58418acfe5fe2cd144d8116362c17bfe2173a9fb68ab59af0"
    },
    {
      "name": "architecture",
      "state": "succeeded",
      "command": "cargo xtask architecture",
      "evidence_ref": "local:architecture:c0e0aebe9b7a8443b26cdb032c4570c955b9dc2cbb395ecb23471f7b441fe176"
    },
    {
      "name": "msrv",
      "state": "succeeded",
      "command": "cargo xtask msrv",
      "evidence_ref": "local:msrv:d1140d0d9e344caef0e6c2f31b629571920fa2dc6fc94a6051e5584caad06c07"
    },
    {
      "name": "supply-chain-policy",
      "state": "succeeded",
      "command": "cargo deny --all-features check",
      "evidence_ref": "local:supply-chain-policy:f1a0fca39d4280363937aabd77783990ea6480bd9ca257816de3b68fc8efa845"
    },
    {
      "name": "advisories",
      "state": "succeeded",
      "command": "cargo audit",
      "evidence_ref": "local:advisories:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    },
    {
      "name": "release-build",
      "state": "succeeded",
      "command": "cargo build --locked --release --package agent-vesper-acp --package agent-vesper-tui --package vesper-web-fetch --package vesper-sandbox --features agent-vesper-acp/docker,agent-vesper-tui/docker,agent-vesper-acp/swarm,agent-vesper-tui/swarm,agent-vesper-acp/bridge,agent-vesper-tui/bridge,agent-vesper-tui/voice-kokoro,agent-vesper-tui/voice-flm",
      "evidence_ref": "local:release-build:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }
  ],
  "main_gates": [
    {
      "name": "pull-request-validation",
      "head_sha": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "run_id": 37531129680,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 324058982,
          "run_id": 37531129680,
          "attempt": 1,
          "job_id": 112500487293,
          "workflow_name": "pull-request-validation",
          "job_name": "supply-chain",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129680/job/112500487293"
        },
        {
          "workflow_id": 324058982,
          "run_id": 37531129680,
          "attempt": 1,
          "job_id": 112500487771,
          "workflow_name": "pull-request-validation",
          "job_name": "quality",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129680/job/112500487771"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129680"
    },
    {
      "name": "msrv",
      "head_sha": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "run_id": 37531129667,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 324058985,
          "run_id": 37531129667,
          "attempt": 1,
          "job_id": 112500488569,
          "workflow_name": "msrv",
          "job_name": "rust-1-88",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129667/job/112500488569"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129667"
    },
    {
      "name": "five-target-foundation",
      "head_sha": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "run_id": 37531129660,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 324058986,
          "run_id": 37531129660,
          "attempt": 1,
          "job_id": 112500487936,
          "workflow_name": "five-target-foundation",
          "job_name": "macos-intel",
          "platform": "macos-15-intel",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129660/job/112500487936"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37531129660,
          "attempt": 1,
          "job_id": 112500488371,
          "workflow_name": "five-target-foundation",
          "job_name": "macos-apple-silicon",
          "platform": "macos-15",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129660/job/112500488371"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37531129660,
          "attempt": 1,
          "job_id": 112500488386,
          "workflow_name": "five-target-foundation",
          "job_name": "windows-x86_64",
          "platform": "windows-2025",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129660/job/112500488386"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37531129660,
          "attempt": 1,
          "job_id": 112500488394,
          "workflow_name": "five-target-foundation",
          "job_name": "linux-x86_64",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129660/job/112500488394"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37531129660,
          "attempt": 1,
          "job_id": 112500488409,
          "workflow_name": "five-target-foundation",
          "job_name": "linux-arm64",
          "platform": "ubuntu-24.04-arm",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129660/job/112500488409"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129660"
    },
    {
      "name": "web-driver",
      "head_sha": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "run_id": 37531129659,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 351737017,
          "run_id": 37531129659,
          "attempt": 1,
          "job_id": 112500487833,
          "workflow_name": "web-driver",
          "job_name": "Native namespace Hive",
          "platform": "ubuntu-22.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129659/job/112500487833"
        },
        {
          "workflow_id": 351737017,
          "run_id": 37531129659,
          "attempt": 1,
          "job_id": 112500488066,
          "workflow_name": "web-driver",
          "job_name": "Contained browser linux-x86_64",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129659/job/112500488066"
        },
        {
          "workflow_id": 351737017,
          "run_id": 37531129659,
          "attempt": 1,
          "job_id": 112500488177,
          "workflow_name": "web-driver",
          "job_name": "Contained browser linux-aarch64",
          "platform": "ubuntu-24.04-arm",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129659/job/112500488177"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37531129659"
    }
  ],
  "changed_version_files": [],
  "changed_repair_files": [],
  "failures": [],
  "repair_attempts": [],
  "repair_admissions": {},
  "retry_budget": {
    "full_gate_limit": 1,
    "full_gate_used": 0,
    "infrastructure_limit": 1,
    "infrastructure_used": 0,
    "targeted_diagnostic_limit": 2,
    "targeted_diagnostic_used": 0,
    "repair_attempt_limit_per_family": 2
  },
  "state_changes": [],
  "transitions": [
    {
      "sequence": 1,
      "at": "2026-10-06T20:21:06.463179605Z",
      "from": "idle",
      "to": "preparing",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "release intent accepted by RRC",
      "evidence_refs": [],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 2,
      "at": "2026-10-06T20:21:06.463696826Z",
      "from": "preparing",
      "to": "local_verification",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "local release verification required before candidate creation",
      "evidence_refs": [],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 3,
      "at": "2026-10-06T21:03:37.943542442Z",
      "from": "local_verification",
      "to": "candidate_ready",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "local release verification passed",
      "evidence_refs": [
        "local:version-preparation",
        "local:workspace-verify:ddca515d4796381703dcbe24fe0da1b3a9eee891da977ec77cf02c49c8adcd0f",
        "local:acceptance:636e0452e16201d58418acfe5fe2cd144d8116362c17bfe2173a9fb68ab59af0",
        "local:architecture:c0e0aebe9b7a8443b26cdb032c4570c955b9dc2cbb395ecb23471f7b441fe176",
        "local:msrv:d1140d0d9e344caef0e6c2f31b629571920fa2dc6fc94a6051e5584caad06c07",
        "local:supply-chain-policy:f1a0fca39d4280363937aabd77783990ea6480bd9ca257816de3b68fc8efa845",
        "local:advisories:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "local:release-build:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 4,
      "at": "2026-10-06T21:03:41.427896242Z",
      "from": "candidate_ready",
      "to": "remote_gate_running",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "exact-commit remote gates dispatched",
      "evidence_refs": [
        "origin/main@0857f02b8dbdc2a867cdb93bf86a03559fb7555b"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 5,
      "at": "2026-10-06T21:03:42.130237476Z",
      "from": "remote_gate_running",
      "to": "waiting_for_matrix",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "required workflow runs have not appeared for the exact candidate SHA",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 6,
      "at": "2026-10-06T22:01:35.970765083Z",
      "from": "waiting_for_matrix",
      "to": "remote_gates_green",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": 324058982,
      "run_id": 37531129680,
      "job_id": 112500487293,
      "reason": "all exact-commit required gates are terminal and green",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 7,
      "at": "2026-10-06T22:01:39.866093818Z",
      "from": "remote_gates_green",
      "to": "tagging",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": 324058982,
      "run_id": 37531129680,
      "job_id": 112500487293,
      "reason": "annotated immutable tag verified at exact green commit",
      "evidence_refs": [
        "git:tag:ce8430990d897404b18b98646daeb45f065951d6"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 8,
      "at": "2026-10-06T22:01:39.866542340Z",
      "from": "tagging",
      "to": "publishing",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": 324058982,
      "run_id": 37531129680,
      "job_id": 112500487293,
      "reason": "release publication started after exact-commit gates",
      "evidence_refs": [
        "git:tag:v0.24.7"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 9,
      "at": "2026-10-06T22:15:54.256129056Z",
      "from": "publishing",
      "to": "published",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": 324058982,
      "run_id": 37531129680,
      "job_id": 112500487293,
      "reason": "release assets and metadata published and verified",
      "evidence_refs": [
        "github:run:37537909556",
        "github:release:v0.24.7"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 10,
      "at": "2026-10-06T22:15:58.502999519Z",
      "from": "published",
      "to": "post_release_closeout",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": 324058982,
      "run_id": 37531129680,
      "job_id": 112500487293,
      "reason": "post-release main-health closeout started",
      "evidence_refs": [
        "rrc:verified-publication-closeout"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 11,
      "at": "2026-10-06T22:15:59.474515519Z",
      "from": "post_release_closeout",
      "to": "waiting_for_matrix",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "post-release closeout refreshes current main gates",
      "evidence_refs": [],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 12,
      "at": "2026-10-06T22:16:28.752892585Z",
      "from": "waiting_for_matrix",
      "to": "complete",
      "source_commit": "0857f02b8dbdc2a867cdb93bf86a03559fb7555b",
      "workflow_id": 324058982,
      "run_id": 37531129680,
      "job_id": 112500487293,
      "reason": "post-release main is green; published release remains unchanged",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    }
  ],
  "metrics": {
    "ci_wait_millis": 3960000,
    "model_active_millis": 0,
    "retries_rejected": 0,
    "repeated_fingerprints_blocked": 0,
    "speculative_reruns_prevented": 0
  },
  "receipt": {
    "report": "/tmp/vesper-rrc-closeout-fix/docs/foundation/release-v0.24.7-closeout.md",
    "registry_url": "https://github.com/agentclientprotocol/registry/pull/539",
    "registry_blob": "54ce9686307d71f2973b2353cedb589579461c52"
  }
}
```

## Deviations, unresolved items and readiness effect

Upstream PR merge remains maintainer-owned. Foundation fixtures do not establish live provider effectiveness. This report certifies the observed release and closeout, not complete implementation/PRD parity. The published tag and assets remain immutable.
