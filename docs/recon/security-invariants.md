# Security Invariants

Status: COMPLETE

## Scope

These invariants are release gates, not implementation suggestions. Evidence paths refer to the frozen Python source.

## Authority and permission invariants

1. Policy denial wins over every session mode, reviewer, plugin, workflow, and user convenience (`agent.py:4805-4840`).
2. Nested workflow steps are evaluated individually; a wrapper cannot launder a denied operation (`agent.py:4808-4836`).
3. Read Only never permits a destructive or discovered MCP tool (`agent.py:4866-4876`).
4. Plan Mode cannot mutate except its explicit `.agent` plan artifact allowance; its MCP exception must be revisited but preserved until approved (`agent.py:4845-4860`).
5. Bypass skips approval only after policy evaluation (`agent.py:4862-4864`).
6. Ask failures deny. Smart review can only auto-allow an exact safe verdict over redacted data; it never auto-denies or expands authority (`agent.py:4882-5092`).
7. Plugins, hooks, workers, cron, browser, and MCP never create authority beyond the parent session/policy.

## Credential invariants

- `ZAI_API_KEY`/`Z_AI_API_KEY` environment values precede stored credentials; terminal entry is hidden; stored credentials are user-only (`config.py:711-775`; `cli.py:21-38`).
- Keys never enter stdout, stderr, logs, ACP updates, telemetry, failure corpora, indexes, hook payloads, worker prompts, smart-approval packets, or custom quota hosts.
- Provider quota monitoring attaches auth only to official HTTPS hosts in the hardcoded allowlist (`glm_client.py:136-147`, `:399-453`).
- Child command/hook/cron/clipboard/browser environments remove API-key/token/secret/password/private/access-key/credential suffixes and SSH agent access (`tools.py:127-135`; `hooks.py:14-38`; `cron_scheduler.py:64-72`; TUI tests).
- Error objects and URLs must be sanitized before display/logging; Rust `reqwest::Error::without_url` is relevant because URLs may embed sensitive query data.

## Filesystem invariants

- Every model-controlled path is canonicalized before authorization and must remain under one of the session roots (`tools.py:1165-1192`).
- Symlinks cannot escape because authorization is against the resolved target.
- Additional roots are explicit session inputs, never inferred from arbitrary tool arguments.
- Binary/NUL/invalid UTF-8 data is rejected consistently by text tools (`tools.py:1129-1141`).
- Project memory, learned skills, references, scripts, vision paths, plugin manifests, worktrees, and promoted failure cases perform their own containment checks in addition to the generic sandbox.
- Atomic writes use same-directory temporary files, fsync where durability matters, private permissions, and replace. Rust must not weaken this on Windows; ACL behavior must be documented.
- Transactional multi-file writes validate every candidate before first mutation and restore prior bytes on failure (`tools.py:1657-1727`).

## Process invariants

- Command children run with scrubbed environment and workspace cwd.
- A timeout/cancellation kills the entire descendant tree and reaps it; stdout/stderr drain cannot deadlock cleanup (`tools.py:1975-2082`).
- POSIX creates a new session/process group. Windows uses Job Object `KILL_ON_JOB_CLOSE` when available (`os_sandbox.py:138-209`).
- Linux Bubblewrap dies with parent, unshares PID, uses fresh `/tmp` and HOME, read-only system binds, workspace-only writable binds, and optional network namespace (`os_sandbox.py:79-119`).
- macOS Seatbelt is deny-by-default with explicit system/workspace/tmp/network rules (`:34-65`, `:120-124`).
- Windows Job Objects are process containment only. Required filesystem or network isolation fails closed rather than claiming security (`:125-135`).
- Hooks are direct argv, never shell strings; executable bytes must match the configured SHA-256 (`hooks.py:41-77`).

Current ambiguity: ordinary `run_command` still uses a shell string (`tools.py:1995-2015`). The Rust design should separate argv execution from an explicitly shell-authorized tool, while retaining semantic parity fixtures.

## Untrusted-context invariants

- Stored instructions/memory/skills are scanned before they enter trusted prompt regions; suspicious sections are replaced, not merely annotated (`security.py:89-95`).
- Tool, file, MCP, browser, reference, recall, and worker outputs are bounded and enclosed in a source-labelled `<untrusted_context>` delimiter (`security.py:98-112`; `agent.py:2609-2613`).
- Promptware scanning detects instruction override, authority impersonation, prompt/credential exfiltration, sensitive-file exfiltration, hidden HTML instructions, and invisible direction controls (`security.py:22-86`).
- Detection is defense-in-depth only and cannot replace containment, environment scrubbing, policy, or approval.
- Learning rejects secret-shaped and promptware content before persistence (`memory.py`; tests `test_memory.py`).

## Persistence/redaction invariants

- Private state directories are 0700 and files 0600 where POSIX supports modes.
- Session FTS excludes system messages, bounds each indexed message to 32K, and redacts private keys, Bearer values, and common credential assignments (`session_store.py:27-39`, `:115-164`).
- Telemetry stores fixed metadata fields, schema 1, no prompt/output/reasoning/command/raw session identity (`telemetry.py:22-97`; tests across learning suites).
- Failure corpus stores coarse suffixes, hashed project identity, failure class and metadata—not bodies/paths/secrets (`failure_corpus.py:43-107`).
- Cron artifacts redact and cap output/error (`cron.py:623-643`).
- Checkpoints exclude `.env*`, credentials, SSH/private-key material and large/ignored content (`checkpoints.py:51-64`, `:350-408`).
- Session reasoning persistence remains controlled by `GLM_ACP_PERSIST_REASONING`; changing its default requires privacy approval (`config.py:669-682`).

## Plugin/publisher invariants

- Plugin manifest schema is exactly supported version 1; IDs/paths are traversal-safe.
- Only data extensions `.json`, `.md`, `.toml`, `.yaml`, `.yml` may be packaged; executable/symlink content is rejected (`plugins.py:287-320`).
- Manifest bytes and content hashes are verified before install/activation.
- Install is staged and atomically swapped, with backup restoration on failure (`plugins.py:321-363`).
- Ed25519 signature verification binds exact manifest bytes to an explicitly trusted publisher key; signature-required mode fails closed (`plugins.py:33-133`, `:369-425`).
- Trust store and private signing key are private; public key may be 0644.

## Browser/mobile/media invariants

- Browser automation is a permission-gated MCP preset with a fixed allowlist and bounded accessibility/console/network/screenshot output (`mcp.py`, `tests/test_extensions.py`).
- Vision files are resolved inside workspace before remote access (`agent.py:3733-3741`).
- Mobile approval defaults to loopback, requires explicit public bind/URL, uses immediate pairing state, and routes only the active permission request (`mobile_server.py:51`; `tests/test_mobile_server.py`, `test_tui.py`).
- Voice transcription is local Whisper; audio is not sent to the provider (`voice.py:40-169`).
- Clipboard helpers execute only on explicit user action with scrubbed environment, one-second timeout, and bounded content (`tui.py:239-359`; `tests/test_tui.py`).

## Checkpoint/rollback invariants

- Checkpoints never touch repository Git state (`checkpoints.py:1-7`, `:142-164`).
- Auto-checkpoint is off by default and bounded by hard maxima (`checkpoints.py:26-49`, `:165-205`).
- Objects are content-addressed, compressed, private, and deduplicated.
- Rollback compares current hash to the recorded post-mutation hash for every path before restoring; conflict means no overwrite.
- Rollback failure restores the pre-rollback bytes where possible and reports any incomplete recovery.
- Legacy schema-1 snapshots are removed only after verified schema-2 migration.

## Cross-platform gates

| Platform | Required proof before release |
|---|---|
| Linux | Bubblewrap capability detection; required-mode failure; PID/network namespaces; descendant kill; symlink/root tests |
| macOS | Seatbelt detection/profile enforcement; required-mode failure; process-group kill; clipboard/notification behavior |
| Windows | Job assignment and kill-on-close; explicit “no filesystem isolation” status; required-mode refusal; path/ACL/rename semantics |

## Tests required before Rust implementation

1. Table-driven permission-precedence tests for every mode/effect/tool class.
2. Symlink race/TOCTOU tests using directory handles or platform-safe open primitives.
3. Secret corpus through logs, errors, FTS, telemetry, hooks, cron, workers, MCP, and exports.
4. Process-tree fixtures with grandchildren that hold pipes open.
5. Sandbox backend conformance tests on real CI hosts.
6. Plugin tamper/signature/publisher/key-substitution cases.
7. Checkpoint conflict and injected write-failure rollback.
8. Malicious promptware and delimiter-escape cases.
9. Mobile approval replay/session-confusion tests.
10. Browser/MCP output and environment isolation tests.

## Migration implication

Place enforcement in dedicated `vesper-policy` and `vesper-security` crates with typed decisions and capabilities. Providers and frontends receive decisions/events; they do not implement policy. Use capability handles for filesystem/process/MCP actions so an `Arc<dyn Provider>` cannot mutate the workspace directly.

## Unresolved risk

TOCTOU remains possible between `Path.resolve` authorization and later path open/write in the Python implementation. Rust should use descriptor-relative/openat-style containment where feasible. That is an intentional security strengthening, not observable-behavior drift.
