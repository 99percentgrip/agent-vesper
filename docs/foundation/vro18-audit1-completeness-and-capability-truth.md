# VRO-18 Audit 1 — field-failure repair and browser authentication acceptance

**Date:** 2026-09-26

**Audit baseline:** `184288197966846cc637eb84e0f8823dbfb62487`

**Released baseline:** `v0.24.0` / `bd49ce69f4e34a64822245e67bdeee2503e1ce4d`

**Working branch:** `audit/vro18-major-1` in `/tmp/agent-vesper-vro18-audit1`
**Verdict:** **OPEN — functional findings repaired; candidate remote workflow gates not run**

## Objective

Repair and prove native xAI/SuperGrok browser authentication without weakening
the already repaired ordinary xAI request path, device fallback or billing-mode
isolation. This report supersedes the earlier Audit 1 browser classification
based on a manually copied, terminal-truncated URL. Audit 2 did not begin. No
push, tag, release, installation or change to Alex's installed application
occurred.

No authorization code, access/refresh token, PKCE verifier, key, cookie,
one-time device code or credential record is present in this evidence.

## Artifact provenance

| Lane | Path | Version | SHA-256 | Provenance / selection | State root |
| --- | --- | --- | --- | --- | --- |
| User-tested installed TUI | `/home/Alex/.local/share/agent-vesper/agent-vesper-tui` | `0.24.0` | `106147156f8e1a04be017a29f626febdbe7fcc95a9cf6a20176c9650ace0cbe0` | Byte-identical to published Linux x86_64 TUI at release commit `bd49ce69…`; xAI / Grok session / `grok-4.7` / high / Code | `/home/Alex/Projects/agent-vesper/.agent-vesper` plus native credential manager |
| Exact v0.24.0 release TUI | `/tmp/vesper-v0240-extract.Sjl1O1/agent-vesper-acp/agent-vesper-tui` | `0.24.0` | `106147156f8e1a04be017a29f626febdbe7fcc95a9cf6a20176c9650ace0cbe0` | Archive SHA-256 `bc47a787f40001830871cdf973aecb3bf7cebc0de2f93bd96b6779b17b6015eb`; release commit `bd49ce69…`; same selections | Original user state plus native credential manager |
| Exact v0.24.0 release ACP | `/tmp/vesper-v0240-extract.Sjl1O1/agent-vesper-acp/agent-vesper-acp` | `0.24.0` | `0a5ec78b020f160d0af543f13128014f994426bf0a5a8fee94b2aef061b04997` | Same archive/commit; xAI / Grok session | Native credential manager |
| First browser-repaired live TUI | superseded build in the recorded audit target | `0.24.0` unreleased | `63d1aae100ff53c479077c57571aa8db9b7426fd803b2a8c0b41936b1f672382` | Baseline plus source diff `f94b0439…`; xAI / browser / `grok-4.7` / high / Code | `/home/Alex/Projects/agent-vesper/.agent-vesper` plus native credential manager |
| Final repaired TUI | `/home/Alex/Projects/agent-vesper/.audit-vro18-candidates/2026-09-26/agent-vesper-tui` | `0.24.0` unreleased | `863c205641e59d45a285d677a44f4c5af9f05599b20b44a206b42a4ae71264b9` | Baseline plus production-source diff SHA-256 `1cbd2b713879cc371a76ed127f79ca15c06d7b9763f2bed8d2f6cba87ac558cf`; xAI / browser / `grok-4.7` / high / Code / Bypass | `/home/Alex/Projects/agent-vesper/.agent-vesper` plus native credential manager |
| Final repaired ACP | `/home/Alex/Projects/agent-vesper/.audit-vro18-candidates/2026-09-26/agent-vesper-acp` | `0.24.0` unreleased | `cc37a3f8f7df49f4bedb137622cbe1862f956e4e79678935396481cef9695ce2` | Same source identity; xAI / stored Grok session / `grok-4.7` | Same Vesper-owned native credential class |

The final candidate used Rust 1.95.0; its source also passes Rust 1.88.0 MSRV
checking. `/home/Alex/.cargo/bin/agent-vesper-tui` is a 76-byte wrapper with
SHA-256 `1f7f4ed7d5682fdfef77ecc73cdf0671e9cdd1716c11e751939e72e6532a68c9`,
not another application binary.

## Ordinary-turn root cause and preserved repair

The exact installed/release TUI reproduced the ordinary Grok-session failure
with the original state. The first rejected component was stale API-key-only
hosted-tool selection `attachment-search`, projected into a Grok-session request
without a required file ID or HTTPS file URL. `wire::append_hosted_tools`
rejected it before credential resolution and network dispatch.

The repair intersects request-time controls with the selected authentication
surface in both hosts, preserves the full ordinary Code-mode tool registry and
fails closed. Safe validator stages now identify provider/model identity,
output bound, capability requirement, reasoning, system content, message
role/content, tool schema, tool name/count, hosted-tool selection, continuation,
provider extension, structured output and request size without echoing content.

Live and process acceptance remains green: TUI and ACP plain turns pass;
`read_file` and `run_command` each executed exactly once; the production
composition loopback transport was reached exactly once. Grok-session mode did
not resolve `XAI_API_KEY`, including after explicit sign-out with an environment
key present.

## Browser-link defect and repair

The earlier `Missing or invalid client_id` observation is:

> **INVALID TEST — manually copied URL was truncated at terminal wrapping**

The TUI rendered the complete client ID across visual lines, but only its first
visual fragment reached the browser. That result did not test Vesper's complete
OAuth request and does not support an unsupported/blocked/client-classification
conclusion.

The repaired TUI stores an authenticated `BrowserSignInLink` as structured
state. Automatic launch and clipboard copy receive that exact value; neither
reconstructs it from rendered lines. Launch uses argument-based platform
commands (`xdg-open`, `open`, or `explorer.exe`) rather than an interpolated
shell string. The modal provides Open browser, Copy complete link, Use device
code and Cancel, and reports launch failure honestly.

A red-first narrow-terminal regression renders the diagnostic URL at 24 columns
so it wraps over many lines, then exercises the TUI open/copy actions. It proves
the browser and clipboard payloads preserve every byte with no inserted newline,
dropped character or duplicate character, including complete `response_type`,
client ID, redirect URI, scopes, code challenge/S256, state, nonce and referrer.

## Current official protocol comparison

Sources refreshed on 2026-09-26:

- `https://auth.x.ai/.well-known/openid-configuration`
- `https://github.com/xai-org/grok-build` at current and pinned commit
  `f0e3be1100ef5252488e3be8bb0e91cf68d8c305`
- current Grok Build authentication documentation in that repository

| Field | Final Vesper request | Current first-party/discovery contract |
| --- | --- | --- |
| Issuer | `https://auth.x.ai` | Same |
| Authorization endpoint | `https://auth.x.ai/oauth2/authorize` | Same |
| Token endpoint | `https://auth.x.ai/oauth2/token` | Same |
| Client ID | complete `b1a00492-073a-47ea-816f-4c329264a828` | Same public native client |
| Redirect | random `http://127.0.0.1:<port>/callback` | Same loopback shape |
| Scopes | `openid profile email offline_access grok-cli:access api:access conversations:read conversations:write workspaces:read workspaces:write` | Same current set |
| Identity | truthful `referrer=agent-vesper`; Vesper version header | Referrer is application-configurable; current token flow requires client version |
| PKCE / state / nonce | S256 / present / present and validated | Same |
| Token/refresh | authorization code or refresh grant, client ID, redirect/verifier as applicable, client-version header | Same field/header classes; token auth method `none` is advertised |
| Device | device endpoint and RFC 8628 polling with client/surface headers | Same distinct endpoint/flow |

Vesper uses the valid public native-app contract while retaining its own
identity and credential store. It does not spoof `grok-build`, read or copy Grok
Build credentials, or launch Grok Build.

## Live acceptance

The first repaired candidate completed the full real-device sequence: automatic
browser launch, user sign-in, loopback callback, state/nonce/PKCE validation,
token exchange, refreshable Vesper credential storage, model discovery,
`grok-4.7` selection, high-reasoning TUI `hello`, ACP `hello`, one `read_file`,
one `run_command`, logout, browser reauthentication and another post-login turn.

The exact final TUI SHA above was then signed out and tested independently. It
automatically opened the complete browser request, received the callback,
loaded account models, selected `grok-4.7` high/Code and returned a normal text
response to `hello`. The exact final ACP SHA consumed that same credential class
and completed a live `session/prompt` with `stopReason=end_turn`.

Device fallback was independently tested after the browser repair: challenge
issued, verification page opened, user code accepted, polling completed,
credential stored, post-auth TUI turn passed and logout passed. No fixture
substituted for a live row.

## Verification

Commands/results include:

```text
cargo test -p vesper-provider-xai --all-features                         # 46/46 PASS
cargo test -p agent-vesper-tui narrow_auth_modal_preserves_complete_structured_url_for_open_and_copy
cargo test -p agent-vesper-acp --features integration-test-harness --test xai_native  # 3/3 PASS
python3 apps/agent-vesper-tui/tests/xai_plain_turn_pty.py <debug TUI>    # PASS, one dispatch
cargo clippy --workspace --all-targets --all-features -- -D warnings   # PASS
cargo fmt --all -- --check                                              # PASS
cargo run -p xtask --quiet -- architecture                              # PASS, 31 packages
cargo run -p xtask --quiet -- naming-guard                              # PASS, 36 frozen hits
cargo run -p xtask --quiet -- acceptance                                # PASS, 23/23
cargo deny check                                                        # PASS
cargo audit                                                             # PASS, 511 dependencies
cargo +1.88.0 check --workspace --all-targets --all-features            # PASS
```

The build used `/home/Alex/Projects/agent-vesper/.audit-vro18-target`, not the
quota-limited `/tmp` target, and peaked at 22 GiB. An accidentally initiated
default target was stopped immediately and cleaned (415.4 MiB). Final binaries
were preserved outside the disposable target; final cleanup removed 48,474
files and 23.7 GiB.

The candidate five-target and web-driver/native-host workflows were **NOT RUN**:
they require an authorized pushed verification ref, while this work unit
explicitly forbids push. The historical v0.24.0 workflow passes do not substitute
for the repaired source. This is the sole remaining Audit 1 gate and is why the
verdict remains OPEN despite complete functional browser acceptance.

## Deviations and unresolved items

- No push/ref was created, so repaired-candidate five-target and
  web-driver/native-host workflows remain unexecuted.
- Optional separately billed live API-key acceptance was not run; offline and
  live signed-out-barrier tests prove it cannot be selected as a subscription
  fallback.
- The credential-like value pasted into the conversation was not used, echoed,
  stored or included in evidence.

## Final accounting

```text
VRO-18 AUDIT 1: OPEN
ORDINARY xAI TUI TURN: PASS
ORDINARY xAI ACP TURN: PASS
BROWSER URL TRUNCATION ROOT CAUSE: CONFIRMED
BROWSER OPEN USES COMPLETE URL: PASS
COPY ACTION USES COMPLETE URL: PASS
COMPLETE CLIENT ID: PASS
COMPLETE REDIRECT URI: PASS
COMPLETE SCOPES: PASS
PKCE: PASS
STATE: PASS
NONCE: PASS
LIVE xAI BROWSER SIGN-IN: PASS
LIVE xAI DEVICE SIGN-IN: PASS
POST-BROWSER-AUTH TUI HELLO: PASS
POST-BROWSER-AUTH ACP HELLO: PASS
READ_FILE EXACTLY ONCE: PASS
RUN_COMMAND EXACTLY ONCE: PASS
LOGOUT: PASS
BROWSER REAUTHENTICATION: PASS
BILLING-MODE ISOLATION: PASS
API-KEY FALLBACK: NO
TESTED BINARY IDENTITIES: RECORDED
FIVE-TARGET: NOT RUN
PUSH/TAG/RELEASE/INSTALL: NO
VRO-18 STATUS: OPEN
READY FOR AUDIT 2: NO
REMAINING OPEN ITEMS: repaired-candidate five-target CI and web-driver/native-host workflow on an authorized temporary verification ref
```

Audit 2 did not begin.
