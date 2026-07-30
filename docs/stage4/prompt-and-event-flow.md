# Prompt and event flow

Status: COMPLETE

## Flow

ACP prompt blocks are bounded and mapped to existing `ContentPart` contracts.
Text, bounded image data, resource links, and embedded resource descriptors are
accepted; audio and unknown blocks are rejected. Empty input and unsupported
slash commands do not dispatch the provider.

The runtime preserves client message identity, assigns a turn, and emits:
acceptance → provider start → ordered reasoning/content/tool/usage/warnings →
one terminal turn event. ACP maps those events to thought chunks, message
chunks, tool lifecycle updates, usage updates, and one correlated prompt
response.

Tool calls are visible as pending/update/failed ACP tool events. Stage 4 never
executes them. Reasoning is emitted but not persisted as ordinary assistant
history; only assistant-visible content is retained.

## Ordering

Bounded channels apply backpressure instead of dropping visible deltas. Per-turn
sequence numbers are monotonic, control events use a monotonic process sequence,
and terminal barriers prevent response/update reordering. Session IDs on every
notification prevent cross-session leakage.

