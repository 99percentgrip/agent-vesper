# Full Harness Parity Audit

Date: 2026-08-01

## Verdict

The native Rust implementation passes the frozen-oracle implementation-parity
gate. The complete Python `LOCAL_COMMANDS` surface is represented, every
registered Rust command has a concrete route, the GLM catalog and plan choices
come from the production adapter, and the release binaries contain neither the
synthetic provider nor the debug-only unsigned-plugin loader.

This verdict means implementation parity against the pinned Python oracle at
`bf4d4287e2e3320aa3f09015f678e6169d520045`. It does not invent a second
production provider or claim that a credential-free verification run made a
billable Z.ai request. Z.ai is the only production adapter currently
registered; the provider registry, runtime, and TUI are ready to accept future
real adapters.

## Command-surface proof

An AST audit parsed `glm_acp/tui.py` from the read-only oracle and compared its
`LOCAL_COMMANDS` keys with `ORACLE_COMMAND_SURFACE` in
`apps/agent-vesper-tui/src/commands.rs`:

| Measurement | Result |
| --- | ---: |
| Python oracle commands | 80 |
| Rust registered commands | 83 |
| Missing oracle commands | 0 |
| Duplicate Rust entries | 0 |
| Intentional Rust additions | `/approve`, `/cancel`, `/quit` |

`/export last` is an explicit registry entry and resolves to
`CheckpointOp::SessionExportLast`. Its executor passes only the final assistant
response to `SessionExporter`; full-session `/export` remains a separate path.

The command-registry tests iterate all 83 entries. Any registered entry without
a typed implementation fails as an internal parity violation. There is no
`Deferred` outcome or generic unsupported fallback.

## Real provider surface

The TUI reads model choices and capabilities from the Z.ai adapter catalog.
No model, endpoint, plan, context limit, vision flag, or reasoning mode is
constructed in the renderer.

| Adapter model | Context | Eligible plans | Capability |
| --- | ---: | --- | --- |
| `glm-5.2` | 1,000,000 | Coding, Standard, BigModel | Flagship; deep reasoning |
| `glm-5-turbo` | 200,000 | Coding, Standard, BigModel | Text |
| `glm-4.7` | 200,000 | Coding, Standard, BigModel | Text |
| `glm-5v-turbo` | 200,000 | Standard, BigModel | Vision |
| `glm-4.5v` | 65,536 | Standard, BigModel | Vision |
| `glm-4.6v` | 131,072 | Standard, BigModel | Vision |

| API plan | Production base endpoint |
| --- | --- |
| Coding Plan | `https://api.z.ai/api/coding/paas/v4` |
| Standard API | `https://api.z.ai/api/paas/v4` |
| BigModel (CN) | `https://open.bigmodel.cn/api/paas/v4` |

Direct image input is rejected unless both the chosen plan and model advertise
vision support. The reasoning picker exposes only adapter-supported values;
Deep High and Deep Max are restricted to `glm-5.2`.

## Harness and interaction matrix

| Area | Concrete implementation and audit result |
| --- | --- |
| Agent loop | Multi-turn production `AgentLoop`; provider streaming, tool calls, cancellation, permissions, plan updates, history, and progress events are connected to the TUI and ACP composition roots. |
| Live activity | Provider reasoning/content deltas, tool start/result events, plan changes, and completion status feed dedicated Reasoning, Activity, TODO, and structured run-report panels. |
| Command palette | All 83 commands are scrollable and searchable. Keyboard navigation, Tab completion, Enter execution, and mouse row selection are implemented. Commands with finite choices open nested pickers instead of requiring memorized arguments. |
| Settings | Model, API plan, reasoning, permission, session mode, generation, auxiliary model, and mixture settings use concrete choices. The model and plan lists are adapter-derived. |
| Working tree | F4 cycles real bounded Changes, Git, Diff, Files, and GitHub views using local `git`, `rg`, and `gh` results. |
| Permissions | Ask, Read Only, and Bypass are selectable. Mutating hosted tools use the one-time approval broker; a closed approval channel fails closed. ACP exposes live permission requests. |
| Memory/checkpoints | Typed durable stores back memory, skills, profile, awareness, checkpoints, lineage, sessions, cron loops, copy, CI, and both export variants. |
| MCP/plugins | Typed MCP registry and signed declarative plugin loader. Executable plugin permission is rejected. |
| Images | Attach, image queueing, screenshot capture, model/plan validation, and terminal image rendering are concrete operations. |
| Mobile approval | Bounded HTTP companion with random pair/approval tokens, 120-second approvals, strict request limits, loopback default, explicit public opt-in, and a terminal QR code only for a phone-reachable advertised URL. |
| Voice | F5 invokes Linux `arecord` or macOS `afrecord`, then local Python `faster-whisper` with `GLM_ACP_WHISPER_MODEL` (default `base`, CPU/int8), matching the oracle contract. Missing capture/transcription dependencies produce an error and never fabricated text. |
| Accessibility/input | Screen-reader mode, native mouse toggle, persistent keybind editor, Vim composer mode, app-managed transcript selection, clipboard copy, sound toggle, and clickable footer actions are wired. |
| Packaging | Release archives contain both `agent-vesper-acp` and `agent-vesper-tui`; shell and PowerShell installers validate and install both; uninstallers remove both while preserving credentials and user state. |

## Structural and security gates

- `vesper-checkpoints` has no normal `sqlite` or `rusqlite` dependency.
- Release `vesper-mcp` contains no `load_unsigned_debug` symbol; the method is
  structurally gated by `#[cfg(debug_assertions)]`.
- Release ACP and TUI binaries contain no `vesper-synthetic` or `synthetic-1`
  marker. Synthetic support is test-only.
- `cargo deny --all-features check` passed all advisory, ban, license, and
  source checks.
- `cargo audit` scanned 379 locked dependencies against 1,178 RustSec
  advisories with no vulnerability finding.
- Mobile approval rejects unacknowledged public binding, malformed decisions,
  oversized requests, and oversized advertised URLs.

## Verification record

The final source state passed:

- `cargo xtask verify`
  - formatting
  - workspace strict Clippy (`-D warnings`)
  - all workspace unit/integration/doc tests
  - 76 compatibility scenarios
  - 154 indexed fixture payloads
  - architecture checks across 20 production packages
  - focused GLM, runtime, ACP, and session gates
- `cargo deny --all-features check`
- `cargo audit`
- `cargo build --release --locked -p agent-vesper-acp -p agent-vesper-tui -p vesper-mcp`
- Python AST command comparison: `oracle=80 rust=83 missing=[]`
- release symbol, string, and dependency-tree audits
- local archive checksum verification and dual-binary installer execution
- installed-binary PTY smoke: startup, `/permission` nested picker exposing
  Ask/Read Only/Bypass, keyboard selection, `/quit`, and clean exit

Final release payloads installed for the current user:

| Binary | Installed path | SHA-256 |
| --- | --- | --- |
| ACP | `/home/alex/.local/share/agent-vesper/agent-vesper-acp` | `cbe02788b645d7df62189b47a2f62a867ce79703d6da02d2308b76c4e78d6deb` |
| TUI | `/home/alex/.local/share/agent-vesper/agent-vesper-tui` | `9058c034d4b19bfc9ab39d51c90b8bcb3d8d908c097386b59f3041e4ca27356d` |

Launchers are installed at `/home/alex/.local/bin/agent-vesper-acp` and
`/home/alex/.local/bin/agent-vesper-tui`; both report version `0.1.0`.

## Environment-dependent validation

Repository policy forbids live provider calls during this verification, so no
credential or paid Z.ai request was used. Provider HTTP behavior is covered by
adapter fixtures and tests; an end-to-end account-specific request remains a
runtime smoke test for the user's configured API key and plan.

On this Linux host, `/usr/bin/arecord` is available and `faster_whisper` is not
currently installed. The voice pipeline is real and its missing-dependency path
is truthful, but microphone-to-transcript execution requires installing the
optional local transcription package. This is an environment prerequisite,
not a fallback transcription or mocked feature.
