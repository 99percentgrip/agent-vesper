# VRO-14 production acceptance audit

Date: 2026-09-07. Baseline: released `v0.20.88` / `9ff4695b`.
Status: production repairs and local acceptance passed; exact-commit CI and
public image publication must pass before release. No release tag is created
on the strength of component tests alone.

## Requirement-to-evidence map

| Requirement | Production implementation | Executable evidence |
| --- | --- | --- |
| Persistent browser session | `vesper-web-fetch/src/browser.rs::BrowserSession`, shared `vesper-harness/src/web_runtime.rs` | Real image opens a pipe, creates/attaches a tab, navigates, captures DOM/AX, and executes real input |
| Index-to-node actions | DOM.resolveNode with live backend IDs; runtime object connectivity checks; mouse coordinates and insertText | Real click changes text, selection changes DOM value, typing changes the input, stale removal yields UnknownIndex |
| Snapshot fidelity | Sparse CDP fields, exact ten styles, AX names/roles, listener clickability, shadow/frame linkage, indexed layout joins | Sparse-column regression; real listener-only node indexed; existing recorded/adversarial fixtures |
| Stable safe maps | Session/backend-ID cache, new markers, retirement, bounded context, containment and opaque paint filtering | Stable button number across mutation; password canaries absent; retired controls never reused |
| Render escalation | Fetch → one ephemeral renderer on empty/JS-shell/content-type/999 signals; independent engine gates | Counting renderer tests, titled SPA fixture, no escalation on egress refusal; actual public-page navigation and rendered HTML |
| Sitemap discovery | Robots directives + default sitemap, bounded XML/index traversal, gzip decoding, normalized deduplication | Original sitemap fixtures exercise cycles and duplicates; helper tests cover gzip expansion limits and malformed input |
| Fetch and egress | In-container DNS validation/pinning, redirects, robots, UA, scheme/host/port grants, bounded charset decoding | Route/egress matrix, RFC octet normalization, Windows-1252 regression, metadata round trip |
| Larger bodies | NUL-framed helper chunks below 64 KiB, aggregate ≤512 KiB, actual status and truncation metadata | Real HTTP fetch exceeds 64 KiB and stays within 128 KiB with truncated=true |
| Bounded crawl/output | BFS, normalized visited set, four runtime permits, bounded batch concurrency, 120-second crawl including seed, per-field budgets | Shared-service passive execution/denial tests; corpus goldens; strict argument validation before I/O |
| Lifecycle/containment | Safe Rust duplex port, explicit image probe, bounded frames/deadlines, resource overrides, RAII teardown and 900-second abandoned-container lease | Real deadline failure, reload, scroll, PNG capture, teardown enumeration; sandbox stream tests |
| Both hosts/default off | One shared service construction and runtime ownership; separate render/interact switches; deferred schemas | Existing cross-host registration/discovery/default-off tests; nonzero real backend identity equals fetch route identity |
| Pinned public driver | In-repo Dockerfile pins base manifest indexes and exact headless package; CI tests immutable image IDs on native x86_64/arm64 | web-driver.yml preserves tested archives and IDs; release.yml requires that exact commit and downloads those artifacts without rebuilding |

`vesper-web/src/driver.rs::plan_commands` is explicitly diagnostic-only;
its unresolved hints are never sent to CDP. The production session supplies
complete wire parameters. A test double is not presented as a working engine.

## Local acceptance commands and results

- `cargo xtask verify`: passed (formatting, strict Clippy, architecture,
  fixtures, conformance, host process checks, workspace tests).
- `cargo +1.88.0 test --workspace --all-features`: 87 suites, **1,672 passed,
  0 failed, 20 ignored**. Baseline was 1,657 passed/18 ignored. No test removed
  or weakened; two new real-browser tests are explicit gated acceptance.
- `cargo test --release -p vesper-web --test adversarial -- --ignored`:
  both unchanged performance ceilings passed (100k-node perception <150 ms,
  5k-interactable serialization <50 ms).
- `VESPER_DOCKER_BIN=podman VESPER_WEB_TEST_IMAGE=sha256:8cd9f992c76a9abb08e81e8a5998d15d32f189cf7f2890535fcfe1d021ca52e5 cargo test -p vesper-web-fetch --all-features real_pipe_browser -- --ignored --nocapture`:
  passed, 11.10 seconds, real Linux x86_64 container. This image ID is local
  evidence, not a promised release asset ID.
- Same immutable image, filter `real_navigation_and_chunked_fetch`:
  passed, 22.82 seconds. Explicit public-site acceptance only; no providers
  or user credentials. Default/foundation tests remain offline.
- `podman ps --filter name=agent-vesper-sbx --format '{{.Names}}'` after
  acceptance: empty; no browser containers left behind.

## Deployment and implementation choices

See `../web-tools.md` for checked-image loading, immutable configuration,
engine opt-ins, denial behavior and bounds. The release supplies Linux images
for both architectures used by Docker/Desktop; the namespaces backend has no
configured egress interface and is not advertised as usable internet access.
The web route is WebSandboxPort, shared by fetch/render/interact through one
runtime, rather than the shell executor's differently shaped SandboxRoute.

The pinned gamma source contains **ten**, not eleven, required styles; the PRD
count is corrected without changing the actual allowlist. The PR-0-selected
lenient quick-xml DOM and workspace-owned Rust Markdown converter remain
in production with their byte-exact golden contracts; no claim of a newly
adopted html5ever/html2text dependency is made. Visible iframe stripping now
emits the required origin-free placeholder in the two f11-edge goldens;
other regenerated golden changes normalize whitespace-only blank lines.
Full subresource enforcement proxy, non-HTML engines,
stealth, uploads/download management and multi-tab orchestration remain the
PRD's original explicit non-goals, not newly invented exclusions.

## Release acceptance

Required before tagging: successful canonical (including supply chain),
MSRV, five-target foundation and native dual-architecture web-driver push
runs for the exact version commit. Public assets must include five application
bundles plus both tested driver archives, archive checksums and immutable
image-ID files. Registry publication updates existing PR #539 in place.
CI and publication results belong to the immutable release's notes; local
Linux evidence is not generalized to other platforms before those gates pass.

Primary protocol references:
[DOMSnapshot](https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/),
[DOM](https://chromedevtools.github.io/devtools-protocol/tot/DOM/),
[Target](https://chromedevtools.github.io/devtools-protocol/tot/Target/), and
[RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html).
