# Agent Vesper ACP executable

## Purpose

Compose configuration, the GLM factory, minimal runtime, ACP adapter, stdio
transport, stderr-only tracing, and orderly shutdown.

## Local Contracts

- Contain no session, provider-wire, or ACP-mapping business logic.
- Stdout is exclusively newline-delimited ACP JSON-RPC.
- Tests use loopback endpoints and synthetic credentials only.
- No child process is created by the application, and the application performs
  no filesystem I/O itself.
- Session readers are disabled unless explicitly enabled, use bounded
  filesystem stores, reject unsafe roots, and never create missing roots.
- Session writers are disabled unless explicitly enabled via
  `AGENT_VESPER_ENABLE_SESSION_WRITES`; the application constructs and injects
  `VesperSessionWriter` but delegates every mutation, atomic rename, and
  sidecar generation to `vesper-sessions`. The write root defaults to the Agent
  Vesper read root (`AGENT_VESPER_SESSION_WRITE_ROOT` or
  `AGENT_VESPER_SESSION_ROOT`) and must be absolute with an existing parent;
  `AGENT_VESPER_SESSION_WRITE_MAX_BYTES` bounds the record size.
- Provider selection is a composition-boundary concern resolved before the
  runtime is constructed: `AGENT_VESPER_PROVIDER` (default `glm`; also
  `synthetic`) or the `--provider` CLI flag maps to the matching concrete
  `ProviderFactory` (`GlmFactory` or `SyntheticFactory`). The runtime stays
  provider-neutral; provider-specific configuration, credential overrides, and
  endpoint identity apply only to the selected adapter.
- The default endpoint assigned to freshly created sessions is injected by the
  composition boundary so persisted records carry a stable endpoint identity:
  `zai-coding` for the GLM adapter and `synthetic` for the synthetic adapter.
  The runtime stays provider-neutral.
- The non-default `integration-test-harness` feature may compose generic
  synchronization wrappers, but the default release binary must not contain a
  dispatch gate or scenario behavior.

## Verification

- Run process transcript tests with isolated environment roots.
- Run `process_blockers` with `--all-features`; the guarded test driver is
  unavailable otherwise.
- Verify stdout purity and stderr secret-canary absence.
- Run `cargo test -p agent-vesper-acp --lib --bins` for read-configuration
  tests without invoking process transcript suites.
- Run `cargo test -p agent-vesper-acp --tests --all-features` for the complete
  real-process suite; every persistence vector must prove exact hash, file-set,
  length, and modification-time invariance.

## Child DOX Index

No children.
