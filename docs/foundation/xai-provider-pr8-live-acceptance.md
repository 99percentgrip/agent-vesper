# VRO-18 PR-8 live acceptance and release execution

## Objective

Validate the native xAI adapter against Alex's SuperGrok account, repair only
evidence-backed production defects, then gate the exact release candidate. This
record separates real-account evidence from loopback fixtures and never treats
API-key billing as implied by a Grok subscription.

## Baseline and isolation

- Repository baseline: `c64e78f46065f1fbaae899ab9914f3d9b3f7023d`
  (post-v0.23.6 `main`).
- Isolated branch/worktree: `vro18/pr0` at `/tmp/agent-vesper-vro18`.
- Last committed candidate before the live repairs:
  `837acdfcfc48190532b4e82735fb78ab550771c3`.
- Build target: `/home/Alex/Projects/.agent-vesper-vro18-target`.
- `XAI_API_KEY` was removed from every live command. The selected and observed
  route was `xai-grok-session`; no public API request or API-key fallback was
  permitted.
- The original checkout and installed Agent Vesper were not modified.

No credential, authorization code, refresh token, or authorization header is
recorded in this report, command receipt, fixture, or repository file.

## Real-account matrix

| Case | Evidence | Result |
|---|---|---|
| Browser sign-in | Native ACP browser flow completed and stored a Vesper-owned session; no Grok Build process or foreign credential import | PASS |
| Account discovery | Session proxy returned verified `grok-4.7`, `grok-4.6`, and `grok-4.5`; `grok-4.7` selected with `high` reasoning | PASS |
| Simple streaming turn | Repaired session request completed with `VRO18_LIVE_OK` | PASS |
| Shared tools | One `read_file Cargo.toml` and one `run_command printf VRO18_RUN_OK` executed through the normal shared loop, each exactly once | PASS |
| Multi-turn continuation | Follow-up returned the prior tool value `VRO18_RUN_OK` | PASS |
| Cancellation | Cancel after first activity settled as `cancelled` in about 425 ms; no later content appeared during a 3 s observation | PASS |
| Post-cancel recovery | A later prompt in the same ACP session settled normally in about 8.7 s | PASS |
| Image input | A valid 64×64 PNG reached `grok-4.7` through ACP and returned `IMAGE_OK` | PASS |
| Usage/status | `/usage` identified Grok-account billing, 500K context and per-response usage; unavailable account allowance remained explicit | PASS |
| Hosted Web Search in session mode | Controlled attempt was rejected by the proxy; capability is now omitted and fails closed for session auth | UNSUPPORTED, FAIL-CLOSED |
| Logout | Logout removed the Vesper credential; `--check-auth` returned “credentials are not configured” | PASS |
| Fresh re-authentication | Device-code restoration completed; `--check-auth` passed and a post-login turn returned `VRO18_LIVE_OK` | PASS |
| API-key live route | Alex supplied no separately billed xAI API credential; implementation remains fixture-tested | NOT RUN |

## Red-to-green defects

### Session proxy request headers

The first authenticated model discovery succeeded, but inference returned a
transport failure. Comparison with pinned first-party Grok Build source showed
that the request omitted mandatory proxy protocol fields. The red fixture then
required:

- authenticate-response marker;
- pinned protocol version `1.0.41`;
- truthful `agent-vesper` client identity and interactive mode;
- model override;
- bounded conversation, request, session and agent correlation.

`XaiSession::send_http` now supplies that contract only for Grok-session
requests. Bearer and session-token auth remain separate from API-key mode. The
red header assertions pass, and the same real account then completed the text,
tool, continuation, cancellation and image cases.

### ACP image capability projection

ACP previously constructed an empty capability index for every provider except
Z.ai and OpenAI, so it rejected a supported xAI image before dispatch. The ACP
composition boundary now builds the index from `XaiCatalog`, while the shared
agent capability gate remains provider-neutral. The new composition regression
passes and the 64×64 live image completed.

### Authentication-mode capability intersection

ACP initially displayed API-region, WebSocket, native-compaction and hosted-tool
controls during Grok-session authentication. The live hosted Web Search rejection
proved that public API capability could not be inherited by the subscription
proxy. Authenticated discovery now records the non-secret auth mode; factory
superpowers and ACP controls expose only model and reasoning controls for
Grok-session. API-only controls remain available to API-key mode and keep their
offline transport coverage.

### Foundation test isolation

Once a real session existed in the OS credential store, all-feature ACP process
tests could eagerly discover xAI while starting a synthetic provider. The
integration harness now performs xAI discovery only when xAI is its explicitly
selected initial provider. Production composition still discovers authenticated
xAI models for provider switching. The full affected host/provider suites pass
with the real credential present and make no unintended live request.

## Commands and current results

The current affected-package command is:

```text
CARGO_TARGET_DIR=/home/Alex/Projects/.agent-vesper-vro18-target \
  cargo test -p vesper-provider-xai -p agent-vesper-acp \
  -p agent-vesper-tui --all-features
```

Result: PASS. This includes 60 ACP unit tests, 12 `process_blockers` tests,
161 TUI unit tests (one ignored), 40 xAI adapter tests, and all affected
integration/doc-test binaries reported by Cargo. `cargo fmt --all` and
`git diff --check` also pass at this checkpoint.

Earlier exact offline gates at `837acdfc…` passed workspace all-feature tests,
strict Clippy, architecture, naming, acceptance, RustSec, Cargo Deny and the
Rust 1.88 MSRV workspace. Because the live repairs change production code,
those receipts are baseline evidence only; PR-8 must rerun the required gates
on the final commit and obtain exact-commit CI before release.

## Files

- `crates/vesper-provider-xai/src/transport.rs` and tests: complete session
  proxy headers and live-proven fail-closed transport intersection.
- `crates/vesper-provider-xai/src/discovery.rs` and `factory.rs`: carry the
  authenticated mode into capability projection.
- `apps/agent-vesper-acp/src/lib.rs` and `controls.rs`: xAI catalog-backed
  capability checks, auth-aware controls and offline test isolation.
- Owning DOX, reconnaissance/status/user documentation and this report: durable
  contracts and evidence.

## Deviations and unresolved items

- A one-pixel PNG was structurally valid but rejected by the provider. It was
  replaced by a normal 64×64 fixture; no claim is based on the rejected probe.
- Grok-session hosted tools are not advertised. The public API-key route retains
  explicit opt-in support; session support requires new first-party evidence
  and live acceptance.
- Subscription allowance/balance is unavailable through the verified session
  protocol. Vesper reports that limitation and does not estimate quota.
- Public API-key live acceptance is optional under §30.2 and was not authorized
  by a separately billed credential.
- Final exact-commit local gates, five-target CI and release publication remain
  pending.

## Readiness effect

The SuperGrok reasoning route has real-account proof for discovery, text,
shared tools, continuation, cancellation/recovery, image input, usage/status
and logout/re-authentication, with three live-exposed production defects
repaired red to green. The post-login trace contained only xAI's explicitly
displayable `response.reasoning_summary_text.delta` channel; encrypted reasoning
remained opaque. VRO-18 remains open until exact-candidate release gates complete.
