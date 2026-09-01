# vesper-sandbox

## Purpose

Own the opt-in OS sandbox backends with honest, probed capability reporting
(VRO-13 PR-3, ADR 0022). The library is **100% safe code**; every raw syscall
(`unshare`, `mount`, uid/gid map writes, `fork`, `prctl(PR_SET_PDEATHSIG)`,
`execve`) lives in the dedicated `sandbox_init` supervisor binary spawned
through safe `std::process`.

## Ownership

- `vesper-security` owns `SandboxCapabilities`, `IsolationRequirement`, and
  the fail-closed `satisfies` semantics this crate consumes.
- This crate owns the namespaces backend, the supervisor protocol, the
  credential-free env allowlist, and bounded output handling. It never
  decides policy; it only reports what it can honestly do and refuses
  everything else.

## Local Contracts

- Strictly opt-in: nothing here is constructed unless a tool explicitly
  demands `IsolationRequirement`. With no demand the executor path is
  byte-identical to the pre-sandbox path.
- Capabilities are **probed, never assumed**. `sandbox_init probe` exits 0
  only if user+mount+PID+network namespaces all provision; any failure
  reports `Unavailable` and every isolation demand fails closed
  (`SandboxError::CapabilityUnavailable`).
- One run per provision: `hold` reads a single unit-separator-delimited
  line from stdin, the child runs as PID 1 of the PID namespace, and killing
  the supervisor chains PDEATHSIG → PID-1 death → kernel SIGKILL of every
  namespace member. Teardown is total without unsafe code in the library.
- Environment hygiene: `SandboxSpec` carries an exact allowlist; the
  supervisor clears every inherited variable. The default baseline
  (`baseline_env`) is credential-free — no provider keys, tokens, or
  cognition-root paths ever enter a sandbox.
- Output is capped at `OUTPUT_CAP_BYTES` (64 KiB per stream). Timeouts kill
  the supervisor and report `timed_out: true` rather than hanging.
- `sandbox_init` is the **only** production component allowed `unsafe`. It
  carries `#![allow(unsafe_code)]` + `#![deny(unsafe_op_in_unsafe_fn)]` +
  `#![deny(clippy::undocumented_unsafe_blocks)]`, and every unsafe block
  carries an adjacent `SAFETY:` comment. This is the ADR 0022 exception
  under `crates/AGENTS.md`; no other module may follow it.
- Linux-only: `libc` is a dependency only under
  `[target.'cfg(target_os = "linux")']`. Off Linux the honest stub
  (`UnavailableBackend`) reports everything unavailable and fails closed.

## Work Guidance

- When adding a new backend, implement `SandboxBackend` and report honest
  probed capabilities; never claim strength the platform did not verify.
- When changing the supervisor protocol, update this doc, the binary's
  module docs, and ADR 0022 in the same change.
- The container this repo is usually developed in blocks the `/proc/self/
  uid_map` write, so namespaces integration tests skip on it — this is the
  honest behavior, not a failure to be "fixed" by faking capability output.

## Verification

- `cargo test -p vesper-sandbox`
- `cargo clippy -p vesper-sandbox --all-targets --all-features -- -D warnings`
- `cargo xtask architecture` (enforces the ADR 0022 allowlist)

## Child DOX Index

No children.
