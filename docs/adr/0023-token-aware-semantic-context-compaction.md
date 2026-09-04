# ADR 0023: Token-aware semantic context compaction

## Status

Accepted.

## Context

The migrated agent loop previously bounded history by retaining the newest 256
messages. That protected memory growth but was not semantic compaction: one
large message could overflow a model window, old decisions and verification
evidence disappeared, `/compact` treated its argument as a keep-count, and the
TUI/ACP/VRO paths did not share a transactional replacement contract.

The public product contracts used as the behavioral baseline are OpenAI's
[conversation compaction guide](https://platform.openai.com/docs/guides/compaction),
[Codex CLI slash-command documentation](https://developers.openai.com/codex/cli/slash-commands),
and Anthropic's [Claude Code context-window guidance](https://code.claude.com/docs/en/context-window.md).
They establish automatic and manual semantic compaction as observable product
behavior; proprietary internal algorithms are not treated as knowable or
copied. The frozen Agent Vesper Python oracle remains the compatibility source
for the conservative 3.5-characters-per-token estimate, pressure thresholds,
and semantic evidence categories.

## Decision

1. `vesper-agent::compaction` is the single provider-neutral implementation.
   It estimates system instructions and every content-part type, emits
   60/75/85 percent pressure tiers, reserves response capacity, and starts
   automatic compaction at 85 percent of the active model's advertised window.
   Hosts supply exact catalog limits; unknown limits use a conservative local
   floor and never borrow a larger limit from another provider.
2. Compaction partitions replaceable conversation history from immutable
   system instructions. It summarizes the older prefix and keeps at least the
   newest four messages, expanding backward across leading tool results so an
   assistant tool call and its result batch remain a complete transaction.
3. The summary preserves goal, decisions, fixes, unresolved work, plan, edits,
   commands, verification, and lineage. `/compact [focus]` adds bounded
   semantic guidance; numeric text is guidance, never a truncation count.
4. Summarizer input is bounded, secret-scrubbed, and enclosed as untrusted
   transcript data. Provider reasoning and opaque payloads are excluded. The
   accepted summary is scrubbed again, bounded dynamically for small windows,
   and wrapped in a versioned untrusted context-summary envelope.
5. Summarization routing is auxiliary provider port, then a tool-disabled main
   provider request, then deterministic extracted evidence. GLM validates
   `zai:auxiliary-model` against the active endpoint plan and uses it only for
   auxiliary wire requests. A malformed, empty, oversized, interrupted, or
   failed provider summary falls back without mutating original history.
6. Commit is transactional. The candidate is validated before replacement;
   if the compacted history plus response reserve still cannot fit, the caller
   receives `ContextWindowExhausted` and ordinary provider dispatch does not
   begin. The original history remains owned by the caller on every error.
7. Direct and VRO execution paths in both TUI and ACP use the same core. ACP
   carries validated full-history replacements through its adapter into the
   serialized runtime actor and existing transactional writer. The TUI stores
   provider-working history separately from the complete human-visible display
   transcript, so `/compact` never erases visible conversation.
8. Every summary persists covered message IDs, reason, focus, before/after/
   capacity estimates, evidence coverage, and a bounded 50-entry quality
   lineage in the namespaced message extension. A decline of at least 15
   percentage points produces a visible warning. Pressure notifications are
   shared across agent-loop instances per durable host session
   and reset when pressure falls or compaction succeeds.

## Consequences

- Message count no longer determines safety; a single large multimodal or tool
  payload contributes to pressure before dispatch.
- System/project/cognition instructions retain their normal provider placement
  and cache stability rather than being rewritten into a summary.
- Host-specific presentation remains separate: ACP emits protocol-safe status
  updates, while the TUI retains its full audit transcript and compacts only
  model-visible working history.
- Exact proprietary tokenizer parity is not claimed. The preflight estimator is
  deliberately conservative and provider usage events remain authoritative for
  displayed billing/accounting; the post-compaction fit gate prevents an
  optimistic estimate from authorizing an obviously unsafe request.
- Quality scoring measures deterministic category coverage, not semantic
  truth. It is an observable regression signal; verification and unresolved
  work in the summary remain model-visible evidence, not proof of completion.

## Verification

- `cargo test -p vesper-agent --all-features`
- `cargo test -p vesper-provider-glm --all-features`
- `cargo test -p vesper-runtime --all-features`
- `cargo test -p vesper-acp --all-features`
- `cargo test -p agent-vesper-acp --all-features`
- `cargo test -p agent-vesper-tui --all-features`
- `cargo xtask architecture`
- Workspace formatting, strict Clippy, and release-equivalent verification.
