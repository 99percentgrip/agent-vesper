# GLM Authentication and Endpoint Security

Status: COMPLETE

Credential resolution checks `ZAI_API_KEY`, then legacy `Z_AI_API_KEY`, through
an injectable secret source. Stage 3 does not read stored credential files.
Secrets use `SecretValue`, are exposed only while constructing an HTTP header,
and are absent from Debug, serialization, errors, events, bodies, and fixtures.

Official endpoint identity is exact parsed scheme/host/path identity for Coding
Plan, Standard API, and BigModel CN. Custom HTTP requires explicit development
opt-in and custom inference authentication is explicit. Custom hosts never gain
quota authority. Official quota hosts require HTTPS and use the source-compatible
raw key header; the monitor response is independently bounded and failures do
not alter inference streams.

Evidence: `glm_acp/config.py::get_api_key` lines 764–776;
`glm_acp/glm_client.py::GlmClient.__init__` lines 128–152 and
`query_plan_usage` lines 373–451; source tests
`test_official_quota_response_is_normalized_without_bearer_prefix` and
`test_custom_endpoint_cannot_receive_usage_credentials`. Rust tests cover
precedence, lookalike hosts, custom HTTP, redaction, inference auth, and an
isolated local quota path.

