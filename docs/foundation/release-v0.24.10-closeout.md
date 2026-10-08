# Release v0.24.10 closeout

## Objective and status

Release v0.24.10 completed.

- Commit: 4426004d59882ef680c7ce388b4151360d893e4e
- Local gates: 8/8 passed; current exact-SHA CI: 12 jobs verified.
- Published inventory: 16 assets; required archive/checksum verification passed.
- Current main: 4426004d59882ef680c7ce388b4151360d893e4e (green).
- Registry: https://github.com/agentclientprotocol/registry/pull/539
- Execution report: /tmp/vesper-rrc-closeout-fix/docs/foundation/release-v0.24.10-closeout.md

Publication and registry/report delivery are complete. Full PRD coverage is tracked separately.

## Methods and commands

Native RRC reobserved current main and the complete exact-SHA workflow matrix. Registry delivery used the existing PR branch, an expected-blob contents update and independent readback; no PR was created, replaced or merged. Reports and links remain local.

## Files and exact evidence

```json
{
  "repository": "/home/Alex/Projects/agent-vesper/.git",
  "epoch": "20261008T084400.527663493Z-4426004d5988",
  "version": "0.24.10",
  "release_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
  "current_main": "4426004d59882ef680c7ce388b4151360d893e4e",
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
      "evidence_ref": "local:workspace-verify:19364fc169391abb732ffa9df5dc2d6b7d48038f9980bd39d26b68d4090569a1"
    },
    {
      "name": "acceptance",
      "state": "succeeded",
      "command": "cargo xtask acceptance",
      "evidence_ref": "local:acceptance:264652a81aef0e14140b01d656d5d51ea5fe3bba33332a1726f870726f0552ef"
    },
    {
      "name": "architecture",
      "state": "succeeded",
      "command": "cargo xtask architecture",
      "evidence_ref": "local:architecture:f31d852eb8d38b7430035d6497eacfa39d88c3dae49744fe05f7390a16301eac"
    },
    {
      "name": "msrv",
      "state": "succeeded",
      "command": "cargo xtask msrv",
      "evidence_ref": "local:msrv:9eaf5ed871fe40869812868156af4f0f98e2ce0c6750fd4f87054c16c8199f3c"
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
      "name": "five-target-foundation",
      "head_sha": "4426004d59882ef680c7ce388b4151360d893e4e",
      "run_id": 37759889363,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 324058986,
          "run_id": 37759889363,
          "attempt": 1,
          "job_id": 113253460232,
          "workflow_name": "five-target-foundation",
          "job_name": "linux-x86_64",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889363/job/113253460232"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37759889363,
          "attempt": 1,
          "job_id": 113253460475,
          "workflow_name": "five-target-foundation",
          "job_name": "windows-x86_64",
          "platform": "windows-2025",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889363/job/113253460475"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37759889363,
          "attempt": 1,
          "job_id": 113253460478,
          "workflow_name": "five-target-foundation",
          "job_name": "linux-arm64",
          "platform": "ubuntu-24.04-arm",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889363/job/113253460478"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37759889363,
          "attempt": 1,
          "job_id": 113253460524,
          "workflow_name": "five-target-foundation",
          "job_name": "macos-intel",
          "platform": "macos-15-intel",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889363/job/113253460524"
        },
        {
          "workflow_id": 324058986,
          "run_id": 37759889363,
          "attempt": 1,
          "job_id": 113253460556,
          "workflow_name": "five-target-foundation",
          "job_name": "macos-apple-silicon",
          "platform": "macos-15",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889363/job/113253460556"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889363"
    },
    {
      "name": "web-driver",
      "head_sha": "4426004d59882ef680c7ce388b4151360d893e4e",
      "run_id": 37759889344,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 351737017,
          "run_id": 37759889344,
          "attempt": 1,
          "job_id": 113253459667,
          "workflow_name": "web-driver",
          "job_name": "Native namespace Hive",
          "platform": "ubuntu-22.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889344/job/113253459667"
        },
        {
          "workflow_id": 351737017,
          "run_id": 37759889344,
          "attempt": 1,
          "job_id": 113253459845,
          "workflow_name": "web-driver",
          "job_name": "Contained browser linux-x86_64",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889344/job/113253459845"
        },
        {
          "workflow_id": 351737017,
          "run_id": 37759889344,
          "attempt": 1,
          "job_id": 113253459994,
          "workflow_name": "web-driver",
          "job_name": "Contained browser linux-aarch64",
          "platform": "ubuntu-24.04-arm",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889344/job/113253459994"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889344"
    },
    {
      "name": "pull-request-validation",
      "head_sha": "4426004d59882ef680c7ce388b4151360d893e4e",
      "run_id": 37759889285,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 324058982,
          "run_id": 37759889285,
          "attempt": 1,
          "job_id": 113253459865,
          "workflow_name": "pull-request-validation",
          "job_name": "quality",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889285/job/113253459865"
        },
        {
          "workflow_id": 324058982,
          "run_id": 37759889285,
          "attempt": 1,
          "job_id": 113253460142,
          "workflow_name": "pull-request-validation",
          "job_name": "supply-chain",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889285/job/113253460142"
        },
        {
          "workflow_id": 324058982,
          "run_id": 37759889285,
          "attempt": 1,
          "job_id": 113253460258,
          "workflow_name": "pull-request-validation",
          "job_name": "windows-installer",
          "platform": "windows-2025",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889285/job/113253460258"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889285"
    },
    {
      "name": "msrv",
      "head_sha": "4426004d59882ef680c7ce388b4151360d893e4e",
      "run_id": 37759889260,
      "run_attempt": 1,
      "run_state": "success",
      "jobs": [
        {
          "workflow_id": 324058985,
          "run_id": 37759889260,
          "attempt": 1,
          "job_id": 113253459652,
          "workflow_name": "msrv",
          "job_name": "rust-1-88",
          "platform": "ubuntu-24.04",
          "state": "success",
          "failed_step": null,
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889260/job/113253459652"
        }
      ],
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37759889260"
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
      "at": "2026-10-08T08:44:00.527732603Z",
      "from": "idle",
      "to": "preparing",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
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
      "at": "2026-10-08T08:44:00.528317942Z",
      "from": "preparing",
      "to": "local_verification",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
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
      "at": "2026-10-08T08:44:03.506895991Z",
      "from": "local_verification",
      "to": "resource_deferred",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "Release paused safely — host resource pressure. version-preparation is waiting.",
      "evidence_refs": [
        "local:resource-deferred"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 4,
      "at": "2026-10-08T08:46:02.984642273Z",
      "from": "resource_deferred",
      "to": "local_verification",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "Host capacity recovered — continuing version-preparation.",
      "evidence_refs": [
        "local:resource-recovered"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 5,
      "at": "2026-10-08T09:54:26.702998372Z",
      "from": "local_verification",
      "to": "candidate_ready",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "local release verification passed",
      "evidence_refs": [
        "local:version-preparation",
        "local:workspace-verify:19364fc169391abb732ffa9df5dc2d6b7d48038f9980bd39d26b68d4090569a1",
        "local:acceptance:264652a81aef0e14140b01d656d5d51ea5fe3bba33332a1726f870726f0552ef",
        "local:architecture:f31d852eb8d38b7430035d6497eacfa39d88c3dae49744fe05f7390a16301eac",
        "local:msrv:9eaf5ed871fe40869812868156af4f0f98e2ce0c6750fd4f87054c16c8199f3c",
        "local:supply-chain-policy:f1a0fca39d4280363937aabd77783990ea6480bd9ca257816de3b68fc8efa845",
        "local:advisories:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "local:release-build:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 6,
      "at": "2026-10-08T09:54:30.962851120Z",
      "from": "candidate_ready",
      "to": "remote_gate_running",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": null,
      "run_id": null,
      "job_id": null,
      "reason": "exact-commit remote gates dispatched",
      "evidence_refs": [
        "origin/main@4426004d59882ef680c7ce388b4151360d893e4e"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 7,
      "at": "2026-10-08T09:54:31.695254329Z",
      "from": "remote_gate_running",
      "to": "waiting_for_matrix",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
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
      "sequence": 8,
      "at": "2026-10-08T10:46:36.544903977Z",
      "from": "waiting_for_matrix",
      "to": "remote_gates_green",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": 324058986,
      "run_id": 37759889363,
      "job_id": 113253460232,
      "reason": "all exact-commit required gates are terminal and green",
      "evidence_refs": [
        "github:exact-sha-matrix"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 9,
      "at": "2026-10-08T10:46:40.906732946Z",
      "from": "remote_gates_green",
      "to": "tagging",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": 324058986,
      "run_id": 37759889363,
      "job_id": 113253460232,
      "reason": "annotated immutable tag verified at exact green commit",
      "evidence_refs": [
        "git:tag:4761bebc2f67e798b9b56427c1822883c7b7f58d"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 10,
      "at": "2026-10-08T10:46:40.907397383Z",
      "from": "tagging",
      "to": "publishing",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": 324058986,
      "run_id": 37759889363,
      "job_id": 113253460232,
      "reason": "release publication started after exact-commit gates",
      "evidence_refs": [
        "git:tag:v0.24.10"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 11,
      "at": "2026-10-08T11:03:09.180054962Z",
      "from": "publishing",
      "to": "published",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": 324058986,
      "run_id": 37759889363,
      "job_id": 113253460232,
      "reason": "release assets and metadata published and verified",
      "evidence_refs": [
        "github:run:37765708101",
        "github:release:v0.24.10"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 12,
      "at": "2026-10-08T11:03:14.360468059Z",
      "from": "published",
      "to": "post_release_closeout",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": 324058986,
      "run_id": 37759889363,
      "job_id": 113253460232,
      "reason": "post-release main-health closeout started",
      "evidence_refs": [
        "rrc:verified-publication-closeout"
      ],
      "repair_attempts": 0,
      "full_gate_retries": 0,
      "infrastructure_retries": 0
    },
    {
      "sequence": 13,
      "at": "2026-10-08T11:03:15.339420901Z",
      "from": "post_release_closeout",
      "to": "waiting_for_matrix",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
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
      "sequence": 14,
      "at": "2026-10-08T11:03:44.164985274Z",
      "from": "waiting_for_matrix",
      "to": "complete",
      "source_commit": "4426004d59882ef680c7ce388b4151360d893e4e",
      "workflow_id": 324058986,
      "run_id": 37759889363,
      "job_id": 113253460232,
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
    "ci_wait_millis": 3720000,
    "model_active_millis": 0,
    "retries_rejected": 0,
    "repeated_fingerprints_blocked": 0,
    "speculative_reruns_prevented": 0
  },
  "receipt": {
    "report": "/tmp/vesper-rrc-closeout-fix/docs/foundation/release-v0.24.10-closeout.md",
    "registry_url": "https://github.com/agentclientprotocol/registry/pull/539",
    "registry_blob": "62f7a8a6572bc3d153cb38f6d45996994c9933de"
  }
}
```

## Deviations, unresolved items and readiness effect

Upstream PR merge remains maintainer-owned. Foundation fixtures do not establish live provider effectiveness. This report certifies the observed release and closeout, not complete implementation/PRD parity. The published tag and assets remain immutable.
