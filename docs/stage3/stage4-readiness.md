# Stage 4 Readiness

Status: COMPLETE

Stage 4 may consume `GlmFactory`, `GlmSession`, `GlmCatalog`, and the neutral
provider event stream. Its bounded target is an ACP adapter and minimal
provider runtime that maps commands/events without implementing the agent/tool
loop or persistence.

Inputs already available:

- typed factory/session/catalog/auxiliary ports;
- exact GLM request and continuation behavior;
- bounded streaming with owned cancellation and one terminal outcome;
- endpoint-scoped authentication and separate quota status;
- 21 executable source scenarios and Stage 3 coverage metadata.

All local Stage 3 gates pass. Remote Linux ARM64, macOS Intel/Apple Silicon,
and Windows x86-64 jobs remain CI-validation pending.
