# ADR 0022: Sandbox supervisor as the sole raw-syscall boundary

## Status

Accepted.

## Context

VRO-13 PR-3 (`docs/qm-extraction-prd.md`, Feature 2 — Opt-in Sandboxing)
requires OS-level isolation for tool execution when a flag or scope demands
it. `crates/AGENTS.md` states: "Unsafe code is denied by the current crates.
Future platform exceptions require a dedicated module, safety comments,
review, and ADR update." Creating user/mount/PID/network namespaces,
writing `uid_map`/`gid_map`, bind-mounting, forking, installing
`PR_SET_PDEATHSIG`, and `execve`-ing a payload are raw syscalls with no safe
std API. The security question was not whether to use them, but where they
may live so every existing crate keeps `#![forbid(unsafe_code)]`.

## Decision

1. New crate `vesper-sandbox` (Linux dependency set: `tokio`,
   `vesper-security`, plus `libc` only under
   `[target.'cfg(target_os = "linux")'.dependencies]`). The **library is
   100% safe code** and keeps `#![forbid(unsafe_code)]`.
2. Every raw syscall lives in the dedicated `sandbox_init` supervisor
   **binary** (`crates/vesper-sandbox/src/bin/sandbox_init.rs`), which the
   library spawns through safe `std::process::Command`. The binary is the
   reviewed dedicated-module exception required by `crates/AGENTS.md`; it
   carries `#![allow(unsafe_code)]`,
   `#![deny(unsafe_op_in_unsafe_fn)]`, and
   `#![deny(clippy::undocumented_unsafe_blocks)]`, and every `unsafe` block
   has a `SAFETY:` comment stating the invariant that makes the FFI call
   sound at that point.
3. The library never links unsafe code and never runs syscalls itself; all
   kernel interaction happens inside the supervisor, inside namespaces the
   library process never entered. The parent↔supervisor surface is a fixed
   byte protocol: `probe` prints one capability line; `hold` prints
   `ready <pid>` then reads one unit-separator-delimited run line from
   stdin (`<cwd><US>argv0<US>argv1…`).
4. Capability reporting is probed, never assumed. The backend runs
   `sandbox_init probe` and turns its real outcome into
   `SandboxCapabilities` (`CapabilityStatus::Available`/`Unavailable`/
   `Unknown`). Hosts that forbid unprivileged namespaces report every
   capability `Unavailable`, and `vesper-security`'s
   `SandboxCapabilities::satisfies` denies `IsolationRequirement` demands
   fail-closed. Off-Linux, the honest `UnavailableBackend` stub denies
   every demand. Nothing fakes success.
5. Sandboxing is strictly opt-in and zero-overhead when not demanded: the
   backend is constructed only when a tool explicitly demands
   `IsolationRequirement`. With no demand, the executor path is unchanged
   from the pre-sandbox path. The supervisor is located beside the current
   executable; `VESPER_SANDBOX_INIT` overrides the path for tests and
   embedders.
6. Teardown is total without host-side cleanup: killing the supervisor
   chains `PR_SET_PDEATHSIG` into the PID-namespace init, and the kernel
   SIGKILLs every remaining namespace member; the mount namespace dies
   with its last process. Dropping `SandboxHandle` performs
   kill+wait through safe `Child` APIs.
7. `cargo xtask architecture` machine-enforces the boundary: the library
   crates keep the unsafe prohibition; only the supervisor binary is
   permitted the raw-syscall surface.

## Consequences

- The workspace's "unsafe code is denied" contract survives this feature
  intact and machine-checked, with one audited, purpose-built binary as
  the entire exception surface.
- The syscall surface is auditable in one file, one process boundary away
  from the harness; supervisor failures map to honest error paths
  (`SandboxError::CapabilityUnavailable`/`Provision`/`Run`/`Teardown`)
  rather than partial isolation.
- Supervisor ↔ library protocol changes are visible breaking changes and
  must bump the run-line/version handshake deliberately.
- Sandboxes are single-use (`run` once per provision) and bounded
  (`timeout_seconds`, 64 KiB stdout/stderr caps), preventing
  supervisor-lifetime escalation of a workload.
- The environment inside the sandbox is exactly the allowlist — the fixed
  credential-free baseline (`PATH`, `HOME=/tmp/vesper-sandbox-home`,
  `LANG`, `TERM`, `PAGER`, `GIT_TERMINAL_PROMPT`,
  `DEBIAN_FRONTEND`) or a caller-supplied allowlist. No provider keys,
  tokens, or cognition-root paths are provisioned; authentication stays
  in the harness process.
- Linux-only: non-Linux hosts fail closed. No cross-platform degradation
  and no promised-but-absent platform backends.
- This ADR records the only production raw-syscall exception; any future
  exception requires its own ADR.

## Verification

- `cargo test -p vesper-sandbox` — inline unit tests including stub
  fail-closed behavior and strength-never-upgrades.
- `cargo test -p vesper-sandbox --test sandbox_linux` — namespace
  provision, `id -u == 0` inside the userns, isolation of the writable
  root, timeout kill, and teardown (Linux only; skipped elsewhere).
- `cargo clippy -p vesper-sandbox --all-targets --all-features -- -D warnings`
- `cargo xtask architecture` — unsafe-denial and dependency direction.
