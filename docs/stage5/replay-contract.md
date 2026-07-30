# Stage 5 Replay Contract

Status: COMPLETE

## Visible-history conversion

Legacy history conversion retains valid provider-neutral user/assistant
history and structural tool-call/result pairing for compatibility. Active ACP
replay emits only non-empty user/assistant text, including flattened supported
text blocks.

Replay excludes:

- system messages;
- tool internals and tool results;
- raw provider metadata;
- hidden or preserved reasoning;
- credentials and secret-shaped provider configuration;
- persisted memory, goals, verification, checkpoints, and learning state.

Unsupported data remains bounded compatibility data where safe; replay does not
execute it.

## Deterministic identity

When a legacy record lacks a complete message ID, Stage 5 derives an ID from:

- session identity;
- message ordinal;
- user/assistant role.

The value is stable across loads, role-distinct, collision-resistant within the
session, content-independent, and never written back to the legacy file.

## Required update order

`ReplayPlan` constructs only this order:

1. visible historical user/assistant messages;
2. non-empty display-only plan representation;
3. safe metadata/configuration update;
4. available-command update;
5. lifecycle load/resume response, owned by the caller after delivery returns.

`ReplaySink::accept` resolves only after the ACP writer accepts each update.
`ReplayPlan::deliver` awaits every acceptance sequentially, so the final
lifecycle response cannot overtake replay. Updates are encoded by the adapter
as they are delivered; the replay engine does not duplicate the full encoded
transcript in memory.

## ACP mapping

The ACP adapter maps visible messages, plan, metadata, and available commands
without importing ACP SDK types into `vesper-sessions`. Stable message identity,
role, session ownership, and replay order are preserved. Corrupt session errors
produce a safe lifecycle error rather than terminating the ACP dispatcher.

## Security result

Process transcripts assert that private system, reasoning, tool, and raw-secret
canaries never appear on ACP stdout or stderr. Unknown fields are not replayed
as content. The complete synthetic persistent tree remains byte-for-byte and
timestamp-for-timestamp unchanged.

