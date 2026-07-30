# Direct Dependency Register

Status: COMPLETE for Stage 5 read-only persistence

Versions below are exact workspace requirements and resolved versions in
`Cargo.lock`. All are compatible with the approved Rust 1.88 MSRV.

| Crate | Version | Owners | Purpose/features | License | Sensitivity/platform | Class and alternatives |
| --- | --- | --- | --- | --- | --- | --- |
| `serde` | 1.0.229 | domain, config, provider, security, sessions, testkit, xtask | Stable DTO and future read-only session-format serialization; `derive` | MIT OR Apache-2.0 | Compatibility-critical; platform-neutral | Mandatory. Manual encoding rejected as error-prone. |
| `serde_json` | 1.0.151 | domain, config, provider, sessions, testkit, xtask; security tests | Extension envelopes, bounded legacy decoding, safe metadata extraction, fixture/schema data | MIT OR Apache-2.0 | Handles untrusted bounded metadata; platform-neutral | Mandatory. A custom JSON parser is unjustified. |
| `thiserror` | 2.0.19 | domain, config, provider, security, sessions, testkit | Typed errors without runtime features | MIT OR Apache-2.0 | Error/redaction review required | Mandatory. Manual implementations add noise. |
| `zeroize` | 1.9.0 | security | Best-effort secret-memory clearing; defaults off, `alloc` | MIT OR Apache-2.0 | Security-sensitive; memory copies cannot be universally eliminated | Mandatory for current secret wrapper. `secrecy` was considered but would add another abstraction/dependency. |
| `url` | 2.5.8 | security | Parse then redact endpoints; defaults off, `std` | MIT OR Apache-2.0 | Security-sensitive; IDNA dependencies; platform-neutral | Mandatory. String heuristics are unsafe. |
| `futures-core` | 0.3.33 | provider, testkit | Runtime-neutral `Future`/`Stream` traits; defaults off, `std` | MIT OR Apache-2.0 | No async runtime selected | Mandatory for ports. Tokio is deferred. |
| `jsonschema` | 0.49.2 | testkit | Draft-compatible fixture validation; defaults off (no HTTP/TLS resolution) | MIT | Test-only attack surface; platform-neutral | Mandatory fixture consumer. Hand-validation cannot prove schema conformance. |
| `sha2` | 0.11.0 | sessions, testkit; ACP app tests | Deterministic legacy message identities, authoritative fixture hashes, reusable testkit tree manifests, and process-test disk invariance; defaults off | MIT OR Apache-2.0 | Identity/integrity-sensitive; platform-neutral | Mandatory. Session identities hash only session ID, ordinal, and role; test-only helpers hash synthetic files. |
| `clap` | 4.6.4 | xtask | Maintenance CLI; defaults off, derive/help/error-context/usage/std | MIT OR Apache-2.0 | Developer-only; platform-neutral | Mandatory maintenance convenience. Manual parsing was possible but less robust. |
| `reqwest` | 0.13.4 | provider-glm | Production HTTP; defaults off, `json`, `rustls` | MIT OR Apache-2.0 | Security-sensitive TLS/proxy/redirect behavior; Rust 1.85; five targets | Mandatory for Stage 3. Explicitly configured Rustls, no redirects/retry/proxy. A bespoke HTTP stack was rejected. |
| `tokio` | 1.52.0 | provider-glm, runtime, sessions, ACP, ACP app | Cancellation-aware bounded transport, actors, blocking-I/O isolation, stdio, signals; defaults off with narrowly selected runtime/I/O features | MIT | Task lifecycle and backpressure; supported on all five target families | Mandatory. Session reads acquire an explicit semaphore before `spawn_blocking`; neutral provider ports remain based on futures traits. |
| `tokio-util` | 0.7.17 | runtime | Hierarchical `CancellationToken`; defaults off, `rt` | MIT | Cancellation correctness; platform-neutral | Mandatory. Hand-rolled cancellation was rejected as race-prone. |
| `futures-util` | 0.3.33 | runtime; provider-glm tests | Provider stream polling; defaults off, `std` | MIT OR Apache-2.0 | Platform-neutral | Mandatory in runtime; test convenience in provider-glm. |
| `httpdate` | 1.0.3 | provider-glm | Parse HTTP-date `Retry-After` | MIT OR Apache-2.0 | Small protocol parser; platform-neutral | Mandatory for source-compatible header handling; manual date parsing rejected. |
| `agent-client-protocol` | 2.0.0 | ACP | Official SDK dispatch and stdio framing; defaults off, `unstable_auth_methods`, `unstable_session_fork` | Apache-2.0 | Protocol boundary; crate version is distinct from wire protocol v1 | Mandatory and pinned to the foundation spike result. Raw JSON-RPC implementation was rejected. |
| `tracing` | 0.1.44 | ACP app | Structured startup/shutdown diagnostics | MIT | Security-sensitive fields; stderr only | Mandatory for bounded composition diagnostics. |
| `tracing-subscriber` | 0.3.22 | ACP app | Stderr formatter and environment filter; defaults off | MIT | Must never write protocol stdout | Mandatory composition support; bespoke logging rejected. |

Transitive dependencies are locked and reviewed by Cargo Deny/audit rather than
listed as architectural choices here. HTTP remains confined to the GLM adapter;
the ACP SDK remains confined to `vesper-acp`. No SQLite, Ratatui, MCP SDK,
signing library, or OS sandbox library entered the Stage 5 production workspace.
Stage 5 Part 6 adds no package version. The reusable testkit store builders use
already pinned `serde_json`, `sha2`, and `thiserror`. Cargo Deny 0.20.2 policy
now explicitly bans `rusqlite`, `sqlx`, and `libsqlite3-sys` while Stage 5 is
read-only; xtask independently rejects SQLite dependencies and production
session mutation APIs.
Disposable spikes remain excluded.

The Stage 3 Rustls chain adds OSI/permissive `ISC`, `BSD-3-Clause`, and
`CDLA-Permissive-2.0` licenses; these were reviewed and added to the allowlist.
Duplicate `getrandom`, `r-efi`, and `syn` versions are transitive across the
fixture tooling, AWS-LC build chain, and proc-macro generations. They remain
warnings for review rather than hidden skips.

Primary crate metadata and local source documentation were consulted on
2026-07-30. Context7 was attempted first for Cargo Deny configuration but its
service quota was exhausted; the pinned Cargo Deny 0.20.2 template was used as
the version-exact fallback and the real policy command validated the syntax. No
API-key file or credential was read.
