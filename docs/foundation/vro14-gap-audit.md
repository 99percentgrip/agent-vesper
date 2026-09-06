# VRO-14 implementation and release audit

Status: PARTIAL implementation; local repair gates passed, exact-commit CI pending.
Date: 2026-09-06. Baseline: `a85e79f` (unreleased workspace version 0.20.88).

## Scope and method

Compared `docs/web-oracle-extraction-prd.md` with production call sites,
helper protocol, configuration, tests, and release packaging. Inspected the
failed GitHub MSRV run `34006779788`: the watcher responsiveness test sampled
its counter before joining the worker and could stop it before scheduling.
Canonical and five-target runs for that baseline succeeded.

## Repaired gaps

| Area | Finding | Repair and evidence |
| --- | --- | --- |
| MSRV/TUI test | Zero sweeps observed under scheduler contention; reused PID-named state | Isolated temporary state, handshake inside the real watcher callback, dispatch while the callback is pending, join before reading its counter; retains the 10,000-command/5-second bound |
| Passive tool execution | All five tools unconditionally refused; no transport injection | Shared `WebService` owns a transport and executes fetch, scrape, page-link map, and bounded BFS crawl; offline execution tests exercise each tool |
| Discovery | Deferred tools were invisible to `search_tools`; results injected no schemas | Include eligible web definitions in discovery and return executable schemas via `with_injected_tools` |
| Helper protocol | Emitted JSON lacked `VWMETA:`; redirect count was guessed; empty-stdout errors lost their reason | Shared emission/parser round-trip test, exact redirect count, typed error mapping, last metadata line wins |
| Egress | No resolved-address checks or redirect policy checks; IPv6 private/link-local tests passed for the wrong reason | Validate each DNS answer, pin addresses into the client, disable proxies and automatic redirects, recheck each hop, add HTTPS IPv6 tests |
| Robots | Fetch helper never consulted robots; rule parser mishandled grouped agents and ties | Fetch robots inside the sandbox before page requests; bounded fail-closed failures; grouped-agent precedence, wildcard/end-anchor matching, Allow wins ties |
| Bounds/lifecycle | Ignored route timeout/body cap, charset expansion exceeded the cap, teardown not explicitly awaited | Clamp timeout/body bounds, cap decoded bytes, apply resource limits, await teardown on run success/failure, retain UTF-8 boundaries and truncation evidence |
| Browser opt-in | Interaction registered with the broad web switch | Separate `interact.enabled` gate; missing production driver remains an explicit capability refusal |
| Packaging | Helper omitted and Docker support disabled in release builds | Package `vesper-web-fetch` beside both hosts and enable their existing Docker features |

Pure policy/protocol tests and fixture transports never contact a provider or
live website. The previous `helper_output_parses_into_fetch_response` test
contained only `let _ = ReplayingBackend`; it now exercises the production
output parser with assertions.

## Remaining PRD work (not completion claims)

- Production pipe-CDP session driver, real index-to-node action resolution,
  live AX/listener enrichment, render waterfall, pinned headless image, and
  gated real-browser acceptance. `driver.rs::plan_commands` contains internal
  index hints and incomplete wire parameters; it is not executable CDP.
- Sitemap discovery, gzip sitemap limits, and sitemap-aware map output.
  `web_map` currently describes and returns page links only.
- Full PRD configuration surface (engine/render switches, user agent,
  scheme/host/port patterns), strict RFC robots normalization, and fetch
  chunking above the sandbox's 64 KiB stream cap.
- A supplied digest-pinned Docker image must contain the helper at
  `/usr/local/bin/vesper-web-fetch`; no image is published by this repair.
  The Linux namespaces backend isolates networking but does not configure an
  egress interface. A successful namespace probe is not evidence of usable
  internet connectivity. Browser interaction remains unavailable on both hosts.
- The historical tests comparing two zero-valued sandbox-holder IDs were
  vacuous. The repair checks actual shared service identity, but does not claim
  the PRD's full browser/fetch route parity acceptance.

These are unfinished requirements, not approved exclusions or impossible
features. Release notes must describe the repair as partial web support.

## Deployment inputs

Passive Docker execution uses `[web] enabled = true`,
`AGENT_VESPER_SANDBOX=docker`, and `VESPER_DOCKER_IMAGE=<image>@sha256:<digest>`.
The image needs the release-compatible helper and CA certificates. Missing
daemon, image, executable, or isolation fails explicitly. No image is pulled
or network process started at host boot. Web calls run on blocking workers;
no browser/network operation enters the TUI rendering path.

## Verification

- Focused TUI, web, helper, and shared-service tests: passed locally.
- Workspace all-target/all-feature Clippy with `-D warnings`: passed locally.
- `cargo xtask verify`: passed locally (including architecture, fixture,
  conformance, host process tests, formatting, Clippy, and workspace tests).
- `cargo +1.88.0 test --workspace --all-features`: 87 suites, 1,657 passed,
  0 failed, 18 ignored. No tests removed or weakened; ignored tests remain
  explicit platform/live/performance gates.
- `cargo test --release -p vesper-web --test adversarial -- --ignored`:
  both performance gates passed.
- Initial restricted runs failed because offline loopback listeners were
  denied by the execution sandbox; authorized reruns passed.
- Cargo Audit and Cargo Deny are not installed locally; the required GitHub
  supply-chain job must pass before tagging. Exact-commit canonical, MSRV,
  five-target, and release workflow evidence will be linked in the release.
- Naming-rule scan over added diff lines and this report: no matches.

Primary references for the helper repairs:
[reqwest ClientBuilder](https://docs.rs/reqwest/0.13.4/reqwest/blocking/struct.ClientBuilder.html)
(`no_proxy`, `resolve_to_addrs`, redirect policy) and
[RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html) (robots groups and rules).
