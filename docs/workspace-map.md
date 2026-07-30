# Stage 5 Read-Only Persistence Workspace Map

Status: COMPLETE (local contracts; remote CI pending)

| Package | Responsibility | Workspace dependencies | Explicitly absent |
| --- | --- | --- | --- |
| `vesper-domain` | DTOs, commands/events, compatibility codec | none | SDKs, I/O, policy evaluation |
| `vesper-security` | Authority-free security primitives | none | sandbox/process backends |
| `vesper-config` | Paths, profiles, typed config contracts | domain, security | user-state migration/writes |
| `vesper-provider` | Provider ports/capabilities/request/stream rules | domain | concrete providers, HTTP, auth |
| `vesper-policy` | Pure permission decision model | domain, security | approval transports, smart-review model |
| `vesper-testkit` | Fixture/conformance validation, deterministic fakes, temporary read-store builders, session fixture loading, and file-tree no-write proofs | foundational crates | production runtime logic |
| `vesper-provider-glm` | Z.ai identity/catalog, auth, wire translation, HTTP/SSE, retry/cancel/continuation/quota | domain, provider, config, security; testkit dev-only | ACP, core, tools, persistence, frontend |
| `vesper-runtime` | Provider-neutral supervisor/actors, injected read repository, converted state adoption, bounded events, cancellation, no-tools turns | domain, provider, sessions; testkit dev-only | filesystem implementation, ACP, GLM, tools |
| `vesper-acp` | Official-SDK ACP v1 compatibility, lifecycle/replay, and command/event mapping | domain, runtime | GLM, direct persistence, tools |
| `vesper-sessions` | Read-only ports, legacy and Agent Vesper decoding, metadata/layouts, converted-state seeds, deterministic IDs, replay plans | domain, config | writes, SQLite, runtime, ACP, GLM |
| `agent-vesper-acp` | Thin stdio composition of ACP, runtime, GLM, and explicitly enabled read stores | ACP, config, runtime, sessions, provider, GLM, domain | business logic, persistence writes |
| `xtask` | Verification/coverage/contracts/architecture commands | testkit | harness runtime logic |

## Future crates

Core, later provider adapters, session decoding/writes, tools, checkpoints, MCP,
workers, memory, automation, plugins, observability, TUI, and CLI are deliberately
not created. Each is added only during its owning migration stage.

## Mechanical enforcement

`cargo xtask architecture` validates the allowlisted workspace graph, rejects
unpinned Git dependencies, scans production foundational sources for ACP SDK,
frontend, HTTP, database, spike, testkit, concrete-provider, and domain-I/O
leakage, and relies on Cargo metadata to reject cycles. `cargo xtask contracts
verify` checks complete fixture ownership and Stage 2 implementation references.

The Stage 5 scan additionally rejects filesystem mutation/directory-creation
APIs, writer-shaped session APIs, SQLite, ACP, runtime, and concrete-provider
references from `vesper-sessions`. Cargo metadata and Cargo Deny independently
reject `rusqlite`, `sqlx`, and `libsqlite3-sys`. `cargo xtask sessions verify`
checks Stage 5 coverage plus the session/testkit suites.

Each current crate uses `#![forbid(unsafe_code)]`. Later reviewed platform modules
may use narrow unsafe blocks only after the relevant ADR and safety review.
