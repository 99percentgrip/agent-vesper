# Contained web tools

Both TUI and ACP use the shared, default-off web service. Enable it only in
trusted workspace configuration. Network permission approval still applies;
configuration and tool discovery never grant approval.

## Install the driver

Releases that include driver assets publish an image archive for each Linux
architecture, including Docker Desktop on macOS/Windows. Select `linux-x86_64`
or `linux-aarch64` to match the container daemon, not its host OS. Download
the matching `vesper-web-driver-<architecture>.tar.gz`, `.tar.gz.sha256`, and
`.image-id` assets from the same release as the application.

Verify the archive with `sha256sum --check <archive>.sha256`, then load it
with `docker load --input <archive>.tar.gz`. Copy the complete `sha256:...`
value from `.image-id` into `web.driver.image`. This is the immutable image
configuration ID, not a registry manifest digest. A registry mirror may
instead use `repository@sha256:<manifest-digest>` after explicitly pulling it.
The application checks local image availability and never implicitly pulls.

Use a functioning Docker daemon; Podman's compatible CLI can be selected
with `VESPER_DOCKER_BIN=podman`. Release binaries enable the Docker backend.
Source builds must enable the host's `docker` feature. The Linux namespaces
backend currently has no configured egress interface; it is not a substitute
for the web driver. No unsandboxed fallback exists.

## Workspace configuration

Merge into `.agent-vesper/config.toml`, replacing the explanatory image value
with the exact ID from the release asset:

```toml
[web]
enabled = true
respect_robots = true
user_agent = "agent-vesper"
output_budget_bytes = 98304
allowlist = ["https://example.com", "https://*.example.com:443"]
deny_private_addresses = true

[web.engine.fetch]
enabled = true
[web.engine.render]
enabled = true
[web.interact]
enabled = true
[web.driver]
image = "sha256:<64 lowercase hexadecimal characters from .image-id>"
[web.sandbox]
requirement = "network"
allow_network = true
```

Absent `[web]` registers nothing and performs no browser/daemon work at boot.
Fetch defaults on within an enabled scope; render and interaction default off
and require their own switches. Disabling private-address denial or choosing
an unpinned image is rejected. An empty allowlist permits public HTTP(S)
origins; it never permits private, loopback, or link-local destinations.
`AGENT_VESPER_SANDBOX=off` refuses web execution as well; it never enables an
uncontained fallback. Closing an unopened browser remains a no-op.

## Operations and bounds

- `web_fetch`: bounded raw page body, actual final URL/status internally.
- `web_scrape`: `markdown`, `fit`, `rawHtml`, `links`, plus page metadata and
  per-field density/truncation evidence. Query-aware fit uses BM25 fallback
  from page title, heading, metadata, then a substantial paragraph.
- `web_map`: page links plus robots-declared/default sitemaps, gzip decoding,
  recursive-index deduplication (25 documents, 10,000 URLs), optional ranking.
- `web_crawl`: breadth-first traversal, default 20 URLs/depth 2/concurrency 2;
  hard caps 200/10/4, wall-clock cap 120 seconds including the seed fetch,
  same-origin default, and explicit denial reasons.
- `web_interact`: `navigate`, `click`, `type` (optional `submit`, `clear_first`),
  `select_option`, `scroll`, `back`, `forward`, `reload`, `screenshot`, `close`.
  Actions use stable numbered live nodes, never model-supplied JavaScript.
  New numbers are marked `[new]`; removed numbers are never reassigned.

Fetch may escalate once to a separate ephemeral renderer on JS-shell signals;
policy refusals never escalate. The interactive page is not modified by a
scrape. Operations share four runtime permits; CDP actions have 45-second
deadlines, startup smoke has 30 seconds, and abandoned containers have a
900-second lease. Container startup waits at most 30 seconds and cleanup CLI
waits five seconds.
If a daemon stalls, its finite container lease remains the cleanup fallback.
Output defaults to 96 KiB per field, hard cap 512 KiB; the host's 1 MiB
envelope may impose an additional reported per-field cut.
Oversized screenshots fail explicitly instead of returning corrupt base64.

All DNS, TLS, robots checks, redirects and browser execution occur in the
resource-limited container. CDP uses anonymous pipes, not a debugging TCP
listener. Browser document requests pause for admission; v1 does not provide
a full subresource allowlist proxy. Sensitive form values are masked during
snapshot extraction. Web content remains untrusted, with provenance markers.
Pipe/protocol failure invalidates the session; actions are never replayed.

Missing daemon, image, helper, isolation, robots permission or browser smoke
produces a refusal. Bot walls fail honestly. PDF/OCR, stealth, downloads,
uploads, multi-tab orchestration and cross-run caching are not v1 features.

Acceptance evidence: [implementation audit](foundation/vro14-gap-audit.md).
