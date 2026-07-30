# Frozen Session Schema-1 Compatibility

Status: COMPLETE

## Objective

Provide read/write-free DTOs and codecs for all seven authoritative session
fixture outcomes without creating a session repository or opening user paths.

## Source evidence

`Session.to_dict` persists schema version, workspace, GLM settings, lineage,
permissions, plan/messages, usage, compaction, verification/learning state,
goals, loaded tools, and checkpoint reference at
`glm_acp/agent.py:527-578`. `Session.from_dict` applies legacy defaults and
bounds at `glm_acp/agent.py:580-667`. Reasoning omission is controlled before
serialization at `glm_acp/agent.py:530-535`.

## Implemented DTO

`LegacySessionV1` at `crates/vesper-domain/src/compatibility.rs:43-160`
represents:

- `version`, `cwd`, `model`, `thought_level`, `mode`, `api_endpoint`,
  `generation_profile`, and `auxiliary_model`;
- title plus parent/root lineage;
- permission mode, plan, ordered messages, and token counters;
- context pressure/task/compaction/instruction state;
- verification, awareness, metacognition, deliberation, repository
  intelligence, and meta-learning envelopes;
- goal/subgoal state, mixture mode, loaded tools, and checkpoint reference;
- flattened unknown fields for round-trip preservation.

GLM settings are exposed only through `LegacyGlmSettings`; they are not added to
provider-neutral session structures.

## Defaults and validation

Omitted fields receive source-compatible scalar/list defaults. Validation at
`compatibility.rs:192-225` requires schema 1 and rejects values beyond confirmed
source bounds rather than silently truncating: task context 2,000 bytes, goal
4,000 bytes, 50 proposals/subgoals, 100 instruction/tool names, and 1,000 bytes
per subgoal.

Malformed JSON, unsupported versions, invalid bounds, unsupported mode values,
and invalid identities have distinct typed errors. Unknown fields survive
decode/encode. `to_neutral_header` at `compatibility.rs:250-301` is explicit and
fallible.

## Reasoning

Legacy `reasoning_content` remains inside compatibility messages and is
detectable with `contains_persisted_reasoning`. The neutral conversion uses the
accepted initial `Persist` default. No reasoning is sent to logs, telemetry,
indexes, exports, workers, or hooks by this codec.

## Fixture result

The testkit exercises all seven directories:

1. complete schema 1;
2. minimal legacy/defaults;
3. unknown-field acceptance plus an executable unknown-field round trip;
4. corrupt JSON/null load plus typed malformed result;
5. replay and fork lineage;
6. reasoning enabled;
7. reasoning disabled.

The five full-record results decode and re-encode with exact known-field
equality. Corrupt and unknown-field focused outcomes use dedicated assertions.
No test or codec opens a real session path.

## Deferred behavior

Atomic persistence, locks, corruption quarantine, session index/meta files,
schema migration UX, managed system-prompt regeneration, and actual replay are
owned by the future sessions stage.

