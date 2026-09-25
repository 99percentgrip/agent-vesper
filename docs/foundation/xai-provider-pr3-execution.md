# VRO-18 PR-3 native Grok session execution

Status: **PASS at offline loopback scope — not registered or advertised**  
Date: 2026-09-25

## Objective

Implement Vesper-owned Grok account authentication and the first-party
subscription proxy without a Grok Build runtime dependency or any implicit
switch to separately billed API-key use.

Requirements: [`../vro18-native-xai-provider-prd.md`](../vro18-native-xai-provider-prd.md).  
Predecessor: [`xai-provider-pr2-execution.md`](xai-provider-pr2-execution.md).

## Pinned first-party contract

Implementation was re-derived from `xai-org/grok-build` commit
`f0e3be1100ef5252488e3be8bb0e91cf68d8c305` (Apache-2.0), principally:

- `crates/codegen/xai-grok-login/src/config.rs`;
- `crates/codegen/xai-grok-login/src/device_code.rs`;
- `crates/codegen/xai-grok-login/src/oidc/{login,protocol,refresh}.rs`;
- `crates/codegen/xai-grok-shell/src/remote/model_source/oai.rs`;
- `crates/codegen/xai-grok-shell/src/agent/config.rs`; and
- `crates/codegen/xai-grok-env/src/lib.rs`.

The verified production contract uses the public client ID
`b1a00492-073a-47ea-816f-4c329264a828`, issuer `https://auth.x.ai`, the
first-party browser/device flows, and
`https://cli-chat-proxy.grok.com/v1` with `X-XAI-Token-Auth: xai-grok-cli`.
No source was copied and Grok Build is not a dependency or subprocess.

## Implementation

- `ProviderCredentialPort` gained a provider-neutral browser-login operation.
  Hosts receive only an authorization URL; adapters retain callback and secret
  ownership. Existing providers keep the inert default.
- xAI advertises two explicit methods: `xai-grok-session` and `xai-api-key`,
  with distinct allowance/billing labels.
- Browser login binds a loopback callback, uses PKCE S256, random state and
  nonce, validates the callback state, constrains discovery/token/JWKS and
  account UI origins, validates the signed ID token issuer/audience/expiry/
  nonce, and persists only after completion.
- Device authorization uses the first-party RFC 8628 endpoints, validates the
  verification URL/code, bounds polling and lifetime, implements
  `authorization_pending`, `slow_down`, denial, expiry, and cancellation.
- Credentials are stored only in Vesper's credential boundary, serialized by
  in-process plus OS advisory locks. Refresh is bounded; rotation is persisted;
  logout writes a tombstone. No `~/.grok/auth.json` or browser credential is
  read.
- A selected Grok session and explicit sign-out cannot fall through to
  `XAI_API_KEY`. Storing a key explicitly switches to API billing.
- Session inference uses the first-party proxy Responses route with bearer,
  `X-XAI-Token-Auth`, and `x-grok-model-override`. One 401 may refresh and retry;
  no other billing mode is attempted. Session model discovery uses the proxy
  `/models` shape and remains separate from API discovery.
- US regional routing is rejected for Grok sessions because it is an API-key
  endpoint choice.

`jsonwebtoken 10.3.0` with the repository's existing AWS-LC backend was added
for browser ID-token verification. It stays inside the provider adapter and
uses no native SDK. The signed loopback test uses a locally generated,
test-only RSA key.

## Red to green evidence

The PR-3 anchor was added before the implementation:

```text
cargo test -p vesper-provider-xai \
  descriptor_exposes_separate_session_and_api_billing_modes --all-features
```

Red result: descriptor length `1`, expected `2`. After implementation, the
xAI suite is **27 passed, 0 failed**. Loopback regressions cover signed browser
OIDC success, state mismatch, PKCE and nonce, device pending/slow-down/success,
cancellation, refresh preservation/rotation, session/API billing isolation,
explicit sign-out, proxy headers, one 401 refresh, and normal API-key behavior.

The first dependency choice enabled `jsonwebtoken`'s Rust-crypto backend. The
supply-chain gate rejected its transitive `rsa 0.9.10` with
`RUSTSEC-2023-0071`. The production dependency was changed to AWS-LC, the
direct RSA test dependency was removed, and `cargo deny check` then reported
`advisories ok, bans ok, licenses ok, sources ok`.

## Verification

```text
cargo fmt --all --check
cargo test -p vesper-provider-xai --all-features
cargo clippy -p vesper-provider-xai --all-targets --all-features -- -D warnings
cargo test -p vesper-provider
cargo xtask architecture
cargo xtask naming-guard
cargo deny check
git diff --check
```

Final result: xAI **27/27 PASS**; provider **21/21 PASS**; strict Clippy,
formatting, architecture, naming, supply-chain and whitespace PASS.

## Deviations and unresolved items

No live Grok login or quota was used. Host browser/device controls and provider
registration remain PR-7, so this phase does not advertise xAI. The verified
session protocol exposes no stable authoritative account allowance endpoint;
usage therefore stays explicitly unavailable. Provider-hosted tools,
continuation/citations, native compaction, WebSocket, host composition, live
acceptance, five-target CI and release remain open under PR-4 through PR-8.

## Readiness effect

PR-3 is closed at offline loopback scope. PR-4 may add shared function-call
continuation, encrypted reasoning persistence, response IDs, cache routing,
usage, and citations without changing the billing/authentication boundary.
