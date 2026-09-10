# ADR 0027: Namespace confinement and observed cleanup

## Status

Accepted repair refinement of ADR 0022, under the approved VRO-15 F09/F10 scope.

## Decision

Keep the sole raw-syscall boundary in `sandbox_init` and the existing probe/hold
wire protocol. Capture outer UID/GID before unshare. Build a private tmpfs root
with only a non-recursive writable workspace bind, read-only system trees,
selected devices and private temporary space. The PID-namespace child chroots,
mounts its own procfs, drops all capability sets and enables no-new-privileges
before executing the absolute payload with the exact allowlisted environment.
No other production module gains unsafe code or another backend.

Bound readiness/probe reads and concurrently drain capped stdout/stderr while
observing the execution deadline. Bound post-kill reap and pipe completion;
report uncertainty rather than claiming that a timeout proves physical cleanup.
This supersedes ADR 0022's unconditional Drop/total-cleanup wording. Empty private
root staging directories remain inside the explicitly retained worker artifacts;
namespace mounts disappear only after their last owning process exits.

## Compatibility and security

The protocol, single-use handle and Linux-only capability model are unchanged.
Unavailable namespace operations fail closed. The child cannot see unrelated host
home/workspace trees or regain mount/chroot privileges. The host remains outside
all namespaces. Non-recursive binds deliberately exclude nested host mounts.
Shared containers retain their separate existing Landlock confinement and restore
the bind mount's original owner, never assuming container root maps to the user.
Nonzero shell exits remain failures with their visible output preserved.

## Migration and verification

No user-state migration or automatic activation. Native Swarm remains opt-in.
`crates/vesper-sandbox/tests/namespaces.rs` has the explicit
`namespace_security_and_timeout_acceptance` gate: real confinement, zero effective
and bounding capabilities, saturated stderr, visible partial stdout, bounded
silent descendant timeout and supervisor reap. The native namespace Hive gate
checks all four topologies, permissioned commands, replacement and clean shutdown.
Both gates are required in `web-driver.yml`; unavailable isolation is failure.
`cargo xtask verify` checks the sole unsafe boundary and strict Clippy; Rust 1.88
and five-target CI remain release requirements. Execution evidence is in
`../foundation/vro15-repair-execution.md`.
