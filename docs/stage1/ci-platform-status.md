# Stage 1 CI and Platform Status

Status: CI VALIDATION PENDING

## Local execution

| Host/target | Status | Evidence |
| --- | --- | --- |
| Linux x86-64 | COMPLETE | Workspace format, Clippy, 44 tests, docs, fixtures, architecture, MSRV, audit, and Cargo Deny passed locally |
| Linux process/Bubblewrap spikes | COMPLETE (Stage 0 evidence) | `docs/foundation/process-sandbox-spike.md` |

Injected path-strategy tests for macOS and Windows validate contract construction
but are not real-host proof.

## Configured remote matrix

| Target family | Runner | Stage 1 status |
| --- | --- | --- |
| Linux x86-64 | `ubuntu-24.04` | CI VALIDATION PENDING |
| Linux ARM64 | `ubuntu-24.04-arm` | CI VALIDATION PENDING |
| macOS Intel | `macos-15-intel` | CI VALIDATION PENDING |
| macOS Apple Silicon | `macos-15` | CI VALIDATION PENDING |
| Windows x86-64 | `windows-2025` | CI VALIDATION PENDING |

`.github/workflows/platform-foundation.yml` runs eligible foundational tests,
fixture/index and architecture validation, process conformance, and prepared
platform sandbox checks. `.github/workflows/foundation-spikes.yml` retains the
disposable compatibility-spike matrix. `.github/workflows/msrv.yml` separately
tests Rust 1.88. `.github/workflows/ci.yml` owns pull-request quality and supply
chain checks.

The workflow files parse locally as YAML. They have not run on GitHub, so no
non-Linux target is described as passing. The runner labels were checked against
the current [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
on 2026-07-29.

## Release implications

Remote matrix failures are release-blocking for the affected family. Required
sandbox requests fail closed when a platform cannot provide the requested
strength. Cross-compilation alone cannot upgrade a row to complete.
