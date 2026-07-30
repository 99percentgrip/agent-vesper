# Agent Vesper Architecture

Status: Stage 5 read-only persistence, replay, and governance

Agent Vesper preserves the Native GLM ACP harness’s observable behavior while
redesigning ownership and dependency boundaries for Rust. Stage 5 adds bounded
read-only repository injection, future Agent Vesper decoding, ACP lifecycle
replay, adversarial/process invariants, fixture coverage, and mechanical
writer/SQLite exclusion. It still contains no persistence writer, SQLite
index, agent loop, tool executor, or frontend.

## Dependency direction

```text
vesper-domain
  ↑       ↑          ↑
config  provider   policy
  ↑                  ↑
security ────────────┘

vesper-testkit → all foundational contracts
xtask          → vesper-testkit
vesper-provider-glm → domain + provider + config + security
vesper-runtime      → domain + provider + sessions read/converted contracts
vesper-acp          → domain + runtime
vesper-sessions     → domain + config
agent-vesper-acp    → ACP + config + runtime + sessions + provider ports + GLM
```

`vesper-domain` and `vesper-security` depend on no workspace crate.
`vesper-config` depends only on domain/security; `vesper-provider` only on
domain; `vesper-policy` only on domain/security. Production crates cannot depend
on `vesper-testkit`, `xtask`, frontends, or spikes. `cargo xtask architecture`
enforces the current graph and scans for forbidden SDK/provider/frontend/runtime
leakage.

`vesper-provider-glm` is a leaf adapter. No shared crate depends on it. HTTP,
GLM wire JSON, authentication, retry, continuation wording, and quota behavior
remain inside that crate; neutral consumers see only `vesper-provider` ports.

`vesper-runtime` owns bounded actor channels and accepts an injected read-only
repository without implementing filesystem I/O or knowing ACP/GLM.
`vesper-acp` alone owns official SDK types and protocol-v1
compatibility. Its physical-writer acceptance gate prevents the SDK's internal
queue from defeating slow-consumer backpressure. Only `agent-vesper-acp`
composes the concrete GLM factory and explicitly enabled read stores; its
non-default process driver can wrap the
neutral factory port with synchronization but is excluded from release builds.

`vesper-sessions` owns raw read intents/records, bounded schema-v1 compatibility
decoding, descriptive layouts, safe filenames/metadata, semaphore-gated reads,
pure runtime-state seeds, deterministic message identities, and ACP-neutral
replay plans plus the future Agent Vesper read-only format. It has no dependency
on runtime, ACP, GLM, SQLite, or testkit.
Composite precedence is in-memory, Agent Vesper read store, then legacy.

## Boundaries

- **Domain:** typed identities, ordered content/tool linkage, usage provenance,
  finish/error classifications, versioned bounded extensions, runtime commands,
  scoped/terminal-safe events, and a read/write-free legacy session codec.
- **Provider ports:** typed capability negotiation, requests, small catalog/factory/
  session/auxiliary ports, owned cancellation, stream events, pre-dispatch
  validation, and terminal/no-replay validation. No provider,
  HTTP client, authentication, or ACP type exists here.
- **Configuration:** injected-platform state-root strategy, profiles, secret
  references, provider config envelopes, and an atomic-write port. It performs no
  user-state writes.
- **Security:** secrets, redaction, untrusted-context delimiters, output bounds,
  path/isolation capabilities, and trust classifications. It grants no authority.
- **Policy:** a pure decision evaluator. Deny is absolute, Bypass cannot override
  denial, Read Only cannot authorize mutation, nested steps are evaluated
  independently, channel failure denies, and smart review is advisory.
- **Testkit:** consumes the language-neutral oracle fixtures and provides
  deterministic fakes, temporary legacy/Agent Vesper read-store builders,
  corrupt/truncated records, file-tree hash manifests, and no-write assertions.
  It is not runtime code.

## Future ownership model

There will be one provider-neutral agent loop. Concrete adapters depend on
provider ports, never the core engine. Frontends consume normalized harness
events and commands, never provider clients. Persistence schemas store
provider-neutral content plus versioned namespaced extensions.

Each current ephemeral session is actor-owned. It serializes state transitions
and turns while provider work runs in an owned child. Cancellation is
hierarchical:

```text
application → session → turn → provider/tool/worker
```

Cancellation is terminal and distinct from generic failure. Converted sessions
with unavailable provider/model/endpoint configuration remain inspectable and
replayable but cannot dispatch a new provider turn. Transactional persistent
writes and the full agent/tool loop remain future owners.

## Compatibility and state

Agent Vesper uses independent platform state roots. Its future session root is
described under the platform data root without being created. Legacy default
and named-profile session roots are represented as read-only descriptors and
missing roots are empty. No silent migration, overwrite, deletion, or
project-local rename is authorized. Reasoning kind and retention are explicit;
generic sinks cannot inherit reasoning.

Legacy message IDs are SHA-256-derived from a versioned domain separator,
session identity, original ordinal, and role. Content is not hashed or exposed,
and the legacy record is never rewritten. Replay emits visible user/assistant
text, then plan, metadata/mode, and truthful available commands; the ACP
lifecycle response follows only after physical-writer acceptance.

## Security authority

Providers supply data, not authority. Policy decisions belong to `vesper-policy`;
future process/filesystem/sandbox services own execution. Frontends request
actions but cannot approve beyond policy. Required unavailable isolation fails
closed.

## Behavioral oracle

The 65 source-derived scenarios under `fixtures/` remain tied to source commit
`bf4d4287e2e3320aa3f09015f678e6169d520045`. Eleven clearly labeled synthetic
future-contract vectors cover neutral semantics the source cannot express.
Rust validates all 76 scenarios, event order, canary absence, and 154 payload
hashes. Coverage distinguishes implemented contract semantics from 53 deferred
runtime behaviors with exact future owners.

Stage 4 retains the unchanged corpus and adds machine-readable ACP/runtime
coverage outside the authoritative hash index. Twelve real process transcript
tests exercise ACP stdio through the runtime and production GLM adapter against
loopback only, including the seven Stage 4.1 retry, continuation, interruption,
cancellation, concurrency, serialization, and backpressure vectors.

Stage 5 retains the same 76-scenario/154-payload corpus and adds a coverage map
outside the authoritative index. Seven source session fixtures, applicable ACP
lifecycle vectors, and synthetic compatibility/security contracts map to
read-only decoding, runtime adoption, replay, process transcript, and exact
disk-invariance evidence. Every non-Stage 5 behavior retains a named future
owner.

See [workspace map](workspace-map.md), [ADRs](adr/), and the detailed
[reconstruction proposal](recon/rust-architecture-proposal.md).
