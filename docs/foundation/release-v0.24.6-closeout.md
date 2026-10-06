# Release v0.24.6 closeout

## Objective and status

Release v0.24.6 completed.

- Commit: a56f0ba76525bf4e7e288eefe2dff731cb120914
- Local gates: 8/8 passed; current exact-SHA CI: 11 jobs verified.
- Published assets: 16 verified.
- Current main: a56f0ba76525bf4e7e288eefe2dff731cb120914 (green).
- Registry: https://github.com/agentclientprotocol/registry/pull/539
- Execution report: /home/Alex/Projects/agent-vesper/.worktrees/openai-gpt-6-1-sol/docs/foundation/release-v0.24.6-closeout.md

No new version, tag, installation or documentation-only push was created during closeout. This release result does not certify full RRC PRD parity.

## Methods and commands

Native RRC reobserved current main and the complete exact-SHA workflow matrix. Registry delivery used the existing PR branch, an expected-blob contents update and independent readback; no PR was created, replaced or merged. Reports and links remain local.

## Files and exact evidence

```json
{
  "current_main": "a56f0ba76525bf4e7e288eefe2dff731cb120914",
  "epoch": "20261006T120554.015869590Z-a32d234a9fb2",
  "local_gates": [
    {
      "command": "cargo check --workspace --all-targets",
      "evidence_ref": "local:version-preparation",
      "name": "version-preparation",
      "state": "succeeded"
    },
    {
      "command": "cargo xtask verify",
      "evidence_ref": "local:workspace-verify:1c02c5b6d722448f00cfeab316ee326c9aacfd02091b267ae77fe6ab4fd1c910",
      "name": "workspace-verify",
      "state": "succeeded"
    },
    {
      "command": "cargo xtask acceptance",
      "evidence_ref": "local:acceptance:1a22a2b288f054d9f335f4e83fad82ccb10b1c929a331696ea761f96e398cdae",
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
      "evidence_ref": "local:msrv:5d1233348cea442cd8a26347c9e11e7c749bbb9c247cd80e892fb07d57e11789",
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
      "head_sha": "a56f0ba76525bf4e7e288eefe2dff731cb120914",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275950226,
          "job_name": "rust-1-88",
          "platform": "ubuntu-24.04",
          "run_id": 37465673794,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673794/job/112275950226",
          "workflow_id": 324058985,
          "workflow_name": "msrv"
        }
      ],
      "name": "msrv",
      "run_attempt": 1,
      "run_id": 37465673794,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673794"
    },
    {
      "head_sha": "a56f0ba76525bf4e7e288eefe2dff731cb120914",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275948937,
          "job_name": "Native namespace Hive",
          "platform": "ubuntu-22.04",
          "run_id": 37465673626,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673626/job/112275948937",
          "workflow_id": 351737017,
          "workflow_name": "web-driver"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275949344,
          "job_name": "Contained browser linux-x86_64",
          "platform": "ubuntu-24.04",
          "run_id": 37465673626,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673626/job/112275949344",
          "workflow_id": 351737017,
          "workflow_name": "web-driver"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275949545,
          "job_name": "Contained browser linux-aarch64",
          "platform": "ubuntu-24.04-arm",
          "run_id": 37465673626,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673626/job/112275949545",
          "workflow_id": 351737017,
          "workflow_name": "web-driver"
        }
      ],
      "name": "web-driver",
      "run_attempt": 1,
      "run_id": 37465673626,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673626"
    },
    {
      "head_sha": "a56f0ba76525bf4e7e288eefe2dff731cb120914",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275949225,
          "job_name": "supply-chain",
          "platform": "ubuntu-24.04",
          "run_id": 37465673604,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673604/job/112275949225",
          "workflow_id": 324058982,
          "workflow_name": "pull-request-validation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275949482,
          "job_name": "quality",
          "platform": "ubuntu-24.04",
          "run_id": 37465673604,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673604/job/112275949482",
          "workflow_id": 324058982,
          "workflow_name": "pull-request-validation"
        }
      ],
      "name": "pull-request-validation",
      "run_attempt": 1,
      "run_id": 37465673604,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673604"
    },
    {
      "head_sha": "a56f0ba76525bf4e7e288eefe2dff731cb120914",
      "jobs": [
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275948657,
          "job_name": "linux-x86_64",
          "platform": "ubuntu-24.04",
          "run_id": 37465673577,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673577/job/112275948657",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275948771,
          "job_name": "windows-x86_64",
          "platform": "windows-2025",
          "run_id": 37465673577,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673577/job/112275948771",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275948808,
          "job_name": "macos-intel",
          "platform": "macos-15-intel",
          "run_id": 37465673577,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673577/job/112275948808",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275949003,
          "job_name": "linux-arm64",
          "platform": "ubuntu-24.04-arm",
          "run_id": 37465673577,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673577/job/112275949003",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        },
        {
          "attempt": 1,
          "failed_step": null,
          "job_id": 112275949055,
          "job_name": "macos-apple-silicon",
          "platform": "macos-15",
          "run_id": 37465673577,
          "state": "success",
          "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673577/job/112275949055",
          "workflow_id": 324058986,
          "workflow_name": "five-target-foundation"
        }
      ],
      "name": "five-target-foundation",
      "run_attempt": 1,
      "run_id": 37465673577,
      "run_state": "success",
      "url": "https://github.com/99percentgrip/agent-vesper/actions/runs/37465673577"
    }
  ],
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
    "registry_blob": "6147513a84cc4ed1a93bc9a96d8a3658d4c3d4e7",
    "registry_url": "https://github.com/agentclientprotocol/registry/pull/539",
    "report": "/home/Alex/Projects/agent-vesper/.worktrees/openai-gpt-6-1-sol/docs/foundation/release-v0.24.6-closeout.md"
  },
  "release_commit": "a56f0ba76525bf4e7e288eefe2dff731cb120914",
  "repository": "/home/Alex/Projects/agent-vesper/.git",
  "version": "0.24.6"
}
```

## Deviations, unresolved items and readiness effect

Upstream PR merge remains maintainer-owned. Foundation fixtures do not establish live provider effectiveness. This report certifies the observed release and closeout, not complete implementation/PRD parity. The published tag and assets remain immutable.
