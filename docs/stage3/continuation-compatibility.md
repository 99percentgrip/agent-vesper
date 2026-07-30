# GLM Continuation Compatibility

Status: COMPLETE

Only output-limit completion without tool calls triggers automatic
continuation. Bounded auxiliary calls never continue. The cap is 20 and the
exact adapter-owned prompt is:

> Continue exactly where you left off. Do not repeat or summarize.

The next request contains the accumulated assistant content and preserved
reasoning when required, followed by that user message. A newline is emitted
between visible segments, usage is checked and accumulated, cancellation is
shared by all attempts, and the adapter emits one final terminal result.
Exhaustion maps to `OutputLimit` with continuation-limit metadata.

Evidence: `glm_acp/glm_client.py::stream_completion` lines 169–228 and
`_merge_result` lines 603–620; source integration tests
`test_length_triggers_continuation`, `test_continuation_cap_is_reported`, and
`test_no_continuation_on_tool_calls`. Rust coverage is in `adapter.rs`,
`request.rs`, and the loopback continuation scenarios.

