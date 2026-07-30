# ADR 0005: Direct Process and Shell Execution

Status: ACCEPTED

## Context

The legacy `run_command` accepts shell language, while deterministic execution
and secure supervision require an argv-native contract. The Python cancellation
fixture also proves a descendant-cleanup defect.

## Decision

Future execution has two explicit contracts:

- `run_process`: executable plus argv, no shell interpretation, structured cwd,
  environment, stdin, timeout, and sandbox request.
- `run_shell`: explicitly selected shell language, destructive and
  permission-gated, with deliberate platform quoting.

Legacy `run_command` remains a compatibility alias for `run_shell` through GLM
parity and is clearly labeled in metadata and permission UX. New internals prefer
`run_process`. Rust must fix, not reproduce, the descendant leak.

## Alternatives considered

- Treat shell strings as argv: rejected because it changes semantics.
- Execute argv through a shell: rejected because it expands authority.
- Remove `run_command` immediately: rejected because it breaks parity.

## Consequences

Process authority remains outside providers. Shell interpretation is explicit,
and process-tree ownership is part of the execution contract.

## Compatibility implications

Existing tool requests keep their shell behavior. Deprecation needs a later ADR
and major-version policy.

## Security implications

Policy denial stays absolute; Bypass cannot override it. Cancellation and timeout
must reap descendants and reject post-cancel output.

## Migration implications

Stage 1 defines only isolation and path capability primitives. No process backend
or command tool is implemented here.

## Verification requirements

Run process-tree conformance on each platform and preserve
`tool.command-cancellation` as a negative reference fixture.

## Evidence

- Historical decision: [foundation ADR 0005](../foundation/adr/0005-command-execution.md)
- Reproduced defect: [process/sandbox spike](../foundation/process-sandbox-spike.md)
- Fixture: `fixtures/tools/command-cancellation`
