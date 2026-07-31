# Stage 1 CI and Platform Status

Status: CI VERIFIED (Linux + macOS); Windows build-time optimization in progress

## Local execution

| Host/target | Status | Evidence |
| --- | --- | --- |
| Linux x86-64 | COMPLETE | Workspace format, Clippy, 191 tests, docs, fixtures, architecture, MSRV, audit, and Cargo Deny passed locally |
| Linux process/Bubblewrap spikes | COMPLETE (Stage 0 evidence) | `docs/foundation/process-sandbox-spike.md` |

Injected path-strategy tests for macOS and Windows validate contract construction
but are not real-host proof.

## Remote matrix — verified green runs

Commit `28d3e35` (all spike test fixes in place):

| Target family | Runner | Status | Run |
| --- | --- | --- | --- |
| Linux x86-64 | `ubuntu-24.04` | **VERIFIED** | [pull-request-validation](https://github.com/99percentgrip/agent-vesper/actions/runs/30589197754) · [five-target-foundation](https://github.com/99percentgrip/agent-vesper/actions/runs/30591149757) |
| Linux ARM64 | `ubuntu-24.04-arm` | **VERIFIED** | [five-target-foundation](https://github.com/99percentgrip/agent-vesper/actions/runs/30591149757) |
| macOS Intel | `macos-15-intel` | **VERIFIED** | [five-target-foundation](https://github.com/99percentgrip/agent-vesper/actions/runs/30591149757) |
| macOS Apple Silicon | `macos-15` | **VERIFIED** | [five-target-foundation](https://github.com/99percentgrip/agent-vesper/actions/runs/30591149757) |
| Windows x86-64 | `windows-2025` | BUILD-TIME PENDING | Cargo artifact caching added to resolve compilation timeout |

The MSRV gate ([msrv.yml](https://github.com/99percentgrip/agent-vesper/actions/runs/30589197785))
and the canonical gate ([pull-request-validation](https://github.com/99percentgrip/agent-vesper/actions/runs/30589197754))
both pass on every push.

`.github/workflows/platform-foundation.yml` runs eligible foundational tests,
fixture/index and architecture validation, process conformance, and prepared
platform sandbox checks. `.github/workflows/foundation-spikes.yml` retains the
disposable compatibility-spike matrix. `.github/workflows/msrv.yml` separately
tests Rust 1.88. `.github/workflows/ci.yml` owns pull-request quality and supply
chain checks.

## Spike test fixes applied (commits 10d21f4 → 28d3e35)

1. **Workspace resolution** (10d21f4): Added empty `[workspace]` table to all
   four spikes so `cargo --manifest-path` doesn't error on the `exclude` rule.
2. **Linux namespace probe** (e8309e6): `bwrap_namespaces_available()` skips
   namespace tests with a diagnostic when the host restricts `unshare(CLONE_NEWUSER)`.
3. **macOS `/proc` fix** (e8309e6): Cross-platform `fd_count()` helper returns
   `None` on non-Linux targets; the fd-leak assertion is Linux-only.
4. **Script self-location** (894b502): `macos-conformance.sh` and
   `windows-conformance.ps1` resolve their own directory before running cargo.
5. **macOS symlink resolution** (28d3e35): `pwd -P` resolves the `/var/folders`
   → `/private/var/folders` symlink so `sandbox-exec` write-scope rules match.
6. **Windows build caching** (f32b78b): Added `swatinem/rust-cache@v2` to cache
   Cargo artifacts and increased timeout to 60 minutes.

## Windows build-time constraint

Windows x86_64 compilation of the 13-package workspace exceeds the CI timeout
on every run (cancelled at 35min, then 50min). This is a **build-performance
constraint**, not a test failure — no test has ever failed on Windows. Cargo
artifact caching (`swatinem/rust-cache@v2`) was added to address this; the
first cached run populates the cache, subsequent runs compile incrementally.

## Release implications

Remote matrix failures are release-blocking for the affected family. Required
sandbox requests fail closed when a platform cannot provide the requested
strength. Cross-compilation alone cannot upgrade a row to complete.
