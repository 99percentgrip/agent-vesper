# ADR 0021: Visible project and global cognitive-memory scopes

## Status

Accepted.

## Context

The cognitive-memory slash commands previously wrote every fact to the current
project's `.agent-vesper/cognition/` database. A successful `/remember` did not
name that scope, so users could reasonably assume identity and preference facts
would follow them across projects. Correcting a wrongly scoped memory required
deleting and re-entering it, and there was no combined audit view.

## Decision

1. The TUI composes two independent `vesper-cognition` engines. The project
   engine retains `AGENT_VESPER_COGNITION_ROOT` or
   `.agent-vesper/cognition/`, preserving existing databases unchanged. The
   global engine uses `AGENT_VESPER_GLOBAL_COGNITION_ROOT`, otherwise
   `$XDG_DATA_HOME/agent-vesper/cognition/`, otherwise
   `~/.local/share/agent-vesper/cognition/`.
2. `/remember` uses deterministic smart routing. Identity and stable preference
   signals route globally; repository, runtime, build, and source-path signals
   route to the project. Ambiguous text defaults to the project to avoid leaking
   repository context across workspaces.
3. `--global` and `--project` explicitly override routing; `--local` is an alias
   for `--project`. Every successful save reports the selected scope, location,
   and routing reason.
4. `/recall` and automatic pre-turn recall search both stores. Results include
   scope labels and short IDs; scoped flags restrict explicit recall and forget.
5. `/memories [query]` audits both stores. `/promote <id>` moves project to
   global, and `/demote <id>` moves global to project. Full IDs and unambiguous
   short prefixes are accepted.
6. Storage, extraction, embedding, and search semantics remain owned by
   `vesper-cognition`; no schema change or new dependency is introduced.

## Consequences

- Existing project memories remain readable at their original location.
- Stable user preferences can follow the user without making repository facts
  global by default.
- Routing is explainable and reversible rather than silently successful.
- A promoted or demoted memory is copied to the destination before source
  deletion. If deletion fails, the transcript reports that both copies may
  remain; it never silently loses the destination copy.
- Global memory remains local to the machine and the configured user ID. It is
  not synchronized or sent to a new service.

## Verification

- `cargo test -p agent-vesper-tui scoped_cognition_commands_parse_overrides_and_lifecycle_operations`
- `cargo test -p agent-vesper-tui smart_memory_routing_is_global_for_identity_and_local_for_project_facts`
- `cargo test -p agent-vesper-tui promotion_and_demotion_use_the_destination_id_between_scoped_stores`
- `cargo test -p agent-vesper-tui`
- `cargo clippy -p agent-vesper-tui --all-targets --all-features -- -D warnings`
- `cargo xtask architecture`
