# GLM Request Serialization

Status: COMPLETE

## Contract

`request::serialize_request` validates the neutral request against
`GlmCatalog`, preserves message/content/tool ordering, then produces the
Z.ai chat-completions JSON body. It owns `thinking`, `clear_thinking`,
`reasoning_effort`, `tool_stream`, generation profiles, image blocks, and the
allowlisted `provider.zai` extension namespace. Unsupported required controls
fail before dispatch; declared fallbacks are returned as observable decisions.

The exact continuation sentence is adapter-owned. External continuation
envelopes must be `provider.zai` schema 1 and match that frozen sentence.
Auxiliary serialization disables streaming, tools, thinking, and continuation,
and clamps output to 1–4096 tokens.

## Evidence

- Frozen source: `glm_acp/glm_client.py::GlmClient._do_stream_request`
  lines 519–555 and `stream_completion` lines 169–228.
- Configuration: `glm_acp/config.py::API_ENDPOINTS`, `GENERATION_PROFILES`,
  `THOUGHT_LEVELS` lines 433–470 and 591–628.
- Rust: `crates/vesper-provider-glm/src/request.rs`, with exact body, tools,
  continuation, auxiliary, and profile unit tests.
- Oracle: `fixtures/provider/glm/request-serialization`.

No provider SDK, credential, or HTTP type crosses the neutral request boundary.

