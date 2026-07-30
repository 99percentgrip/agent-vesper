# ACP-Neutral Contract Map

Status: COMPLETE

## Objective

Show that observed ACP protocol-v1 behavior can map to shared Agent Vesper
commands/events without importing ACP SDK or wire types into `vesper-domain`.

## Confirmed source behavior

The frozen agent implements initialization and session operations in
`glm_acp/agent.py:1842-2168`, configuration/mode changes at
`glm_acp/agent.py:2182-2354`, prompt dispatch at `glm_acp/agent.py:2356-2538`,
and cancellation at `glm_acp/agent.py:2540-2554`. Replay emits ordered
`session_update` notifications in `Session._replay_messages`,
`glm_acp/agent.py:6791-6840`. Prompt responses carry `user_message_id` at
`glm_acp/agent.py:2389,2420,2430,2528`.

The 12 source-derived ACP fixtures remain the executable evidence under
`fixtures/acp/`. The disposable official-SDK result remains documented in
`docs/foundation/acp-rust-sdk-spike.md`; Stage 2 does not import that SDK.

## Mapping

| ACP request/update | Shared boundary | Preserved semantics | Future owner |
| --- | --- | --- | --- |
| initialize | `InitializeRuntime` / `RuntimeInitialized` | capability set, authentication descriptors, roots | ACP adapter |
| new session | `CreateSession` / `SessionCreated` | requested/generated identity, roots, revision | ACP adapter + session actor |
| load | `LoadSession` / `SessionLoaded` | identity, extra roots, replay count | ACP adapter + sessions |
| resume | `ResumeSession` / `SessionLoaded` | identity and recovered revision | ACP adapter + sessions |
| list | `ListSessions` / `SessionListProduced` | ordered bounded summaries | ACP adapter + sessions |
| fork | `ForkSession` / `SessionCreated` | parent/root lineage through session metadata | ACP adapter + sessions |
| close | `CloseSession` / `SessionClosed` | explicit closure | ACP adapter + session actor |
| prompt | `SubmitPrompt` / `UserMessageAccepted` | `MessageId`, command/event correlation, ordered content | ACP adapter + core |
| slash command | `ExecuteSlashCommand` | message identity, name, ordered arguments | ACP adapter + core |
| cancel | `CancelTurn` / `TurnCancelled` | session/turn ownership, visible-output flag | ACP adapter + core |
| permission | `PermissionRequested` / `ProvidePermissionDecision` / `PermissionResolved` | request identity and fail-closed outcome | ACP adapter + policy |
| plan/tool/usage updates | typed `HarnessEventPayload` variants | sequence, tool linkage, usage mode/provenance | ACP adapter |
| prompt terminal | `TurnCompleted` or `TurnCancelled` | explicit single terminal state | ACP adapter + core |

The command envelope at `crates/vesper-domain/src/command.rs:116-228` carries
schema, command, correlation, initiator, optional revision, and typed payload.
The event envelope and state validator at
`crates/vesper-domain/src/event.rs:257-359` enforce runtime/session/turn
ownership, monotonic sequence, terminal uniqueness, and no post-terminal event.

## Known compatibility-wrapper issue

`userMessageId` placement is intentionally not encoded in shared DTOs. The
message identity is preserved as `PromptSubmission.message_id` and
`UserMessageAccepted.message_id`; the future ACP wrapper must map it to the exact
SDK/wire location. `fixtures/contracts/acp-message-id-linkage/` makes that
adapter obligation explicit.

## Tests and limits

- `vesper-domain::event::tests` verifies scoped monotonicity and terminal
  uniqueness.
- `vesper-testkit::conformance::tests` verifies command/event correlation and
  message identity.
- `fixtures/coverage-stage2.json` records contract representation for every ACP
  scenario while retaining all protocol dispatch and serialization as deferred.

No ACP wire JSON, SDK callback behavior, stdio server, or authentication runtime
was implemented. Those remain later-stage behavior.

## Migration implication

The future ACP adapter is a translation boundary: ACP SDK request → command, and
event → ACP update. It must not become a second session state machine.

