# Agent Vesper

Agent Vesper is an active migration of the completed Native GLM ACP harness into
a Rust-native, provider-neutral agent architecture. The frozen Python project at
commit `bf4d4287e2e3320aa3f09015f678e6169d520045` is the behavioral reference.

This repository currently contains the Stage 2 contracts, Stage 3 production
Z.ai GLM adapter, and Stage 4 ACP protocol-v1/minimal ephemeral runtime path.
The ACP binary can exercise no-tools GLM turns, but persistence, the agent/tool
loop, and interactive frontends do not exist. It is **not yet the complete
agent harness**.
GLM parity has not been reached, and production OpenAI, Anthropic, Gemini, local
OpenAI-compatible, and other provider adapters have not been implemented.

All seven Stage 4.1 process-level blockers pass locally. Stage 5 now provides
bounded read-only discovery/decoding, runtime store injection, ACP lifecycle
replay, adversarial invariants, deterministic disk-invariance transcripts,
fixture coverage, and enforcement that no session writer or SQLite dependency
is reachable. Transactional writes, repair, migration, and search remain
unimplemented.
Non-Linux-x86-64 host validation is still pending in CI.

## Local verification

The pinned development toolchain is installed automatically by Rustup.

```bash
cargo xtask verify
cargo xtask msrv
cargo xtask fixtures coverage --stage 2
cargo xtask fixtures coverage --stage 3
cargo xtask fixtures coverage --stage 4
cargo xtask fixtures coverage --stage 5
cargo xtask contracts verify
cargo xtask provider glm verify
cargo xtask runtime verify
cargo xtask acp verify
cargo xtask sessions verify
```

The first command runs formatting, Clippy, tests, fixture validation/index
verification, and architecture rules. The MSRV command requires the Rust 1.88.0
toolchain. See [migration status](docs/migration-status.md), [architecture](docs/architecture.md),
and [contributing](CONTRIBUTING.md).

No runtime performance claim is made at this stage.
