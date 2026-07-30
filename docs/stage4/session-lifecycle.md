# Ephemeral session lifecycle

Status: COMPLETE

## Semantics

Stage 4 implements create, list, load, resume, fork, close, and additional roots
only for sessions registered in the current process. Load/resume replay accepted
user and assistant-visible history in order. Missing IDs return not-found; no
disk search, legacy discovery, fallback fabrication, or state write occurs.

Fork copies supported history, provider configuration, usage, modes, and roots;
it assigns a new ID, records the parent, preserves the branch root, and leaves
the parent unchanged. Close cancels active work, joins the actor, emits one
close event, and prevents later session events.

## Intentional temporary difference

The frozen Python harness can load persisted sessions. Stage 4 advertises and
implements only in-process load/resume. Cross-process semantics are deferred to
Stage 5 and are not simulated.

