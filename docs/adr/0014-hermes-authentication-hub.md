# ADR 0014: Hermes Authentication Hub and Native Credential Storage

Status: ACCEPTED

Builds on: [ADR 0013](0013-stage-15-mcp-client-and-plugin-loader.md).

## Context

The command-migration Stage 15 in ADR 0013 completed MCP/plugin parity, but
the TUI still entered the main harness without credentials and deferred the
failure until a provider-backed action. Credential setup also wrote directly
to a plaintext JSON file. The UI-polish milestone also called “Stage 15”
requires a startup authentication screen and native secure storage. This ADR
uses “Hermes” for that presentation milestone and does not renumber or alter
the accepted command-migration stages.

The production registry currently contains one real provider adapter: Z.ai.
Provider-neutral architecture is not permission to advertise unimplemented
OpenAI, Anthropic, Google, or synthetic-provider credentials.

## Decision

1. New crate `vesper-auth` owns provider-neutral credential identities,
   bounded local validation, and persistence. It depends only on
   `vesper-security` among workspace crates.
2. Production storage tries the operating-system credential manager first via
   `keyring` 4.1.6: macOS Keychain, Windows Credential Manager, and Linux
   Secret Service. Calls run on Tokio blocking threads at the TUI composition
   boundary because Linux Secret Service access is synchronous.
3. When native storage is unavailable on Unix, the store atomically writes a
   bounded JSON vault. Newly created parent directories are mode `0700`; the
   temporary and final files are mode `0600` from creation. Existing arbitrary
   parent directories are never chmodded. Windows fails closed if its native
   credential manager is unavailable because POSIX mode bits cannot prove the
   fallback private.
4. Environment variables retain precedence. The Z.ai adapter reads the new
   store next and preserves read-only compatibility with its legacy
   `{"zai_api_key": ...}` file. New writes use the provider-neutral vault
   schema only when the native store is unavailable.
5. `AuthHubState` is a pure state machine. Its secret uses `Zeroizing<String>`,
   debug output reports only length, and rendering projects only `*` masking.
   The responsive modal uses the registered provider descriptors and currently
   exposes only Z.ai.
6. The TUI checks for a locally valid credential before drawing the normal
   conversation loop. Missing or structurally malformed credentials route to
   Hermes in the same terminal. Successful persistence transitions through a
   full redraw without leaving raw mode or the alternate screen.
7. Startup validation is deliberately local: non-empty, bounded, and free of
   control characters. It does not claim that a key is accepted remotely and
   makes no billable/live provider request. This matches the frozen oracle's
   `--check-auth`, which checks configuration presence rather than contacting
   Z.ai. Provider authentication failures remain truthfully surfaced by the
   first live request.

## Compatibility

- `ZAI_API_KEY` remains preferred over `Z_AI_API_KEY` and stored credentials.
- Existing Agent Vesper credential files remain readable.
- `agent-vesper-acp --setup` uses the same native-first store through the GLM
  adapter; its CLI and secret-safe output contract are unchanged.
- The Auth Hub accepts terminal resize, paste, Unicode-safe backspace, arrows,
  Enter, Escape, and Ctrl-C across crossterm-supported terminals.

## Security consequences

- Secrets never enter logs, debug output, status messages, fixtures, or render
  buffers.
- Native keyring account names are namespaced by provider.
- Native storage failure is explicit; fallback is limited to platforms where
  owner-only modes are verifiable.
- Tests never touch a live keyring or provider endpoint.

## Migration consequences

- Adding a production provider now requires a real adapter and an Auth Hub
  descriptor at the composition boundary. Architecture alone does not create
  a visible provider.
- The old plaintext file is deprecated for writes but retained for read
  compatibility. No automatic migration mutates user credentials at startup.

## Verification

- `cargo test -p vesper-auth --all-features` proves bounded round trips,
  secret-safe formatting, and exact Unix `0700`/`0600` permissions.
- `cargo test -p agent-vesper-tui --all-features` proves missing-key startup
  interception, present-key bypass, masked state and render output, and small
  terminal safety.
- `cargo xtask architecture` validates the new dependency direction.
- `cargo xtask verify` runs the full workspace gate without live credentials
  or provider calls.
