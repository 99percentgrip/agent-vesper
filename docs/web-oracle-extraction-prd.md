# PRD — VRO-14: The Web Oracle Extraction

Status: COMPLETE for VRO-14 v1, released as `v0.20.89`. Browser sessions,
render escalation, sitemap discovery and the public pinned driver images have
passed production acceptance and exact-commit release gates. Evidence and
disclosed implementation choices are in `docs/foundation/vro14-gap-audit.md`;
deployment instructions are in `docs/web-tools.md`. Original v1 non-goals remain.
Implementation evidence lands under `docs/foundation/` per `docs/AGENTS.md`.

Reference upstream (the triad), explicitly authorized as trusted data and
cloned locally:

- **web oracle alpha** (the Eyes: workflow) —
  `/home/Alex/Projects/harness-web-oracle-alpha`, pinned
  `329377c7665074b2a89e36717eff26dc99ffacad`.
- **web oracle beta** (the Eyes: filtering) —
  `/home/Alex/Projects/harness-web-oracle-beta`, pinned
  `862f6bccb9c063f49b9d42701baa0eea17a4993f`.
- **web oracle gamma** (the Hands: interaction) —
  `/home/Alex/Projects/harness-web-oracle-gamma`, pinned
  `d5453ae80e11d61c94986d3506d37a829b473c42`.

**Naming rule (binding).** The brand names of the three upstream projects,
their vendors, and their repository owners must never appear in any PRD,
AGENTS.md, README, source comment, fixture, or commit message produced by
this work. They are referenced exclusively as *web oracle alpha*, *web
oracle beta*, and *web oracle gamma*. Where an upstream-internal path would
embed a banned token (gamma's package directory name), this document cites
`<pkg-root>` — defined as the single top-level Python package of web oracle
gamma — instead of the literal path.

---

## 0. Executive Summary

### 0.1 The triad, and what is worth taking

The three upstream projects have already paid the design cost for the three
layers of a complete web entity. Vesper extracts those layers as one native
Rust toolset for the ReAct loop:

1. **Passive workflow mapping (alpha).** A scrape is not one fetch; it is a
   *lifecycle*: URL admission (robots, blocklist, depth) → an engine
   waterfall ordered by capability support and quality → post-engine
   transformation (unwanted-element removal, markdown conversion, link and
   metadata extraction) → bounded formats. A crawl is that lifecycle plus a
   bounded queue with dedupe, sitemap discovery, and per-denial reasons. A
   map is link discovery plus relevance ranking (cosine similarity over
   link text). We take the *lifecycle shape, the waterfall concept
   (degraded to two local engines), the transformer pipeline, and the
   denial-reason discipline*. We do not take the cloud engines, caches,
   queues, or anything multi-tenant.
2. **Intelligent normalization (beta).** Raw HTML is not LLM-ready. Beta's
   contribution is a *pure-logic* content filter family: a pruning filter
   that scores DOM nodes on text density, link density, tag weight,
   class/id weight, and text length, prunes below a fixed or dynamic
   threshold, and emits high-density "fit" markdown; and a BM25 filter that
   ranks chunks against a query (page title/meta/h1 fallback). This is
   100% portable pure computation — no I/O, no model calls — and becomes
   the heart of the Perception Engine.
3. **Agentic interaction (gamma).** Gamma's current generation maps the
   live DOM to a compact indexed representation the model can act on: a CDP
   `DOMSnapshot.captureSnapshot` pass (10 required computed styles),
   enrichment with the accessibility tree, bounds, visibility,
   clickability heuristics (JS click listeners, ARIA roles, form-control
   wrappers), paint-order filtering, and a serializer that assigns stable
   interactive indices backed by a `(session, backend-node-id)` selector
   map with cross-step caching. Actions (click, type, navigate, scroll,
   select) reference indices, not selectors. We take the snapshot→index
   mapping, the stability discipline, and the sensitive-value hygiene, and
   we port them to a sandbox-contained headless browser driven over CDP.

**The synthesis:** two oracles act as the *eyes* (mapping, scraping, and
token-efficient DOM→markdown filtering) and the third as the *hands*
(autonomous headless interaction), unified behind Vesper's existing
provider-neutral tool registry, permission gate, and sandbox boundary.

### 0.2 Explicit non-goals (from all three repositories)

Ignored completely, per the extraction directive and the recon:

- API servers, HTTP controllers, SDKs, and self-hosting deployment surface.
- Billing, credits, rate plans, org/team flags, per-tenant anything.
- Queues, Redis/Postgres persistence, job workers, webhooks, idempotency.
- Cloud render/stealth-proxy engines and anti-bot evasion (proxy pools,
  TLS-client fingerprint spoofing, stealth variants). Vesper is a
  single-user local tool; bot-walled sites fail honestly rather than being
  evaded.
- LLM-extraction transforms (alpha's `llmExtract`/`query`/`summary`).
  Vesper's provider layer already does model-mediated reasoning; the web
  tools return deterministic bounded content.
- PDF/DOCX/image-OCR file engines (v1 is HTML-only; extension path in §6).
- Index/cache services, change tracking, deep research, search indexing,
  ZDR, telemetry, SIEM, threat-protection vendors.
- Cloud browser sessions, session video recording, demo overlays.

### 0.3 Non-negotiable: zero harness degradation

- The **1,420 test floor** (`docs/foundation/vro13-final-audit.md`) must
  not drop. Every PR proves `cargo test --workspace --all-features` ≥ its
  predecessor's count; no test is deleted or weakened to admit a feature.
- **All web tools are opt-in.** Default configuration registers none of
  them; the crate compiles with zero new runtime cost when unused. Network
  code is feature-gated and lives behind ports; nothing web-related
  executes at host boot.
- **ReAct loop and TUI responsiveness are untouched.** No new await points
  on any render path; web executors are ordinary `ToolExecutor` futures
  bounded by timeouts and output caps, offloaded like `run_command`.
- **All headless browser and network operations run inside the
  `vesper-sandbox` boundary** under `IsolationRequirement::Network` with an
  explicit egress grant (§4). There is no unsandboxed network path in any
  web tool, in either host, in any configuration.

### 0.4 Source-of-truth map (upstream → Vesper seam)

| Concept | Upstream location (pinned rev) | Vesper seam |
|---|---|---|
| Scrape lifecycle + engine waterfall | alpha `apps/api/src/scraper/scrapeURL/index.ts`, `engines/index.ts` | `vesper-web` `pipeline` (F1) |
| Feature-flag support score × quality sort | alpha `engines/index.ts` `buildFallbackList` | two-engine ladder (§1.2), fixed table |
| Unwanted-element removal pre-conversion | alpha `scrapeURL/lib/removeUnwantedElements.ts` | `vesper-web` `strip` stage |
| Markdown conversion + post-process | alpha `lib/html-to-markdown*.ts`, Go service | `MarkdownConverter` (Rust-native) |
| Link/metadata extraction | alpha `scrapeURL/lib/extractLinks*.ts`, `extractMetadata.ts` | `LinkExtractor`, `MetadataExtractor` |
| Crawl loop, robots, depth, denial reasons | alpha `apps/api/src/scraper/WebScraper/crawler.ts` | `CrawlPolicy`/`CrawlSession` |
| Sitemap discovery (gzip, limits) | alpha `WebScraper/sitemap.ts` | `SitemapFetcher` (sandboxed) |
| Map relevance ranking (cosine) | alpha `lib/map-cosine.ts`, `map-utils.ts` | `rank_links` (pure) |
| Prompt-injection guard on scraped content | alpha `scrapeURL/lib/promptInjectionGuard.ts` | bounded content marker (§1.7) |
| Pruning content filter | beta `<pkg-root>/content_filter_strategy.py` `PruningContentFilter` | `TextDensityFilter` |
| BM25 content filter | beta `<pkg-root>/content_filter_strategy.py` `BM25ContentFilter` | `Bm25Filter` (pure Rust BM25) |
| Fit-markdown generation | beta `<pkg-root>/markdown_generation_strategy.py` | `fit_markdown` output path |
| html2text conversion options | beta `<pkg-root>/html2text/config.py` | converter options struct |
| CDP DOMSnapshot enrichment | gamma `<pkg-root>/dom/service.py`, `enhanced_snapshot.py` | `SnapshotModel` (F2) |
| Computed-style allowlist (10 styles) | gamma `enhanced_snapshot.py` `REQUIRED_COMPUTED_STYLES` | `REQUIRED_COMPUTED_STYLES` const |
| Sensitive-input redaction at extraction | gamma `enhanced_snapshot.py` `_SENSITIVE_INPUT_TYPES` | snapshot-layer redaction (§2.2) |
| Sensitive-input filtering policy | gamma `enhanced_snapshot.py` `_is_sensitive_input` | field-layer mirroring (§2.2) |
| Clickability heuristics | gamma `<pkg-root>/dom/serializer/clickable_elements.py` | `is_interactive` |
| Indexed serializer + selector map | gamma `<pkg-root>/dom/serializer/serializer.py` | `InteractableMap`, `serialize` |
| Cross-step index stability | gamma serializer cached `(session, backend_node_id)` keys | `SelectorMapCache` |
| Sensitive input redaction | gamma `enhanced_snapshot.py` `_SENSITIVE_INPUT_TYPES` | `redact_sensitive` |
| Action registry | gamma `<pkg-root>/agent/views.py` (`action_names`) | `BrowserAction` enum |
| Sandbox route + egress grant | — (ours) | `SandboxRoute`, `IsolationRequirement::Network` |
| Tool injection + deferred loading | — (ours) | `ToolResult::with_injected_tools`, `defer_loading` |

### 0.5 Definition of done per feature

(a) pure module in the owning crate with unit tests; (b) integration test
against an offline fixture under `fixtures/web-oracle/`; (c) default-off
opt-in switch (`[web]` config section + feature flags); (d) doc note in the
nearest `AGENTS.md`; (e) evidence line in `docs/foundation/vro14-prN-*.md`
cited from the PR description; (f) workspace test count ≥ 1,420.

---

## 1. Feature 1 — The Perception Engine (Eyes)

### 1.1 Problem

`web_search`/`web_reader` (existing first-party MCP presets) return vendor-
processed summaries. The ReAct loop needs *raw, local, token-efficient*
web content: full page text at bounded token cost, link graphs for site
mapping, and multi-page crawls — all without shipping page bytes to a
third party beyond the fetch itself.

### 1.2 Architecture — a pure four-stage pipeline

New production crate `crates/vesper-web` (library, `#![forbid(unsafe_code)]`,
no I/O of its own):

```
Fetch → Render → Strip → Prune → Convert → Extract
```

```rust
pub struct ScrapePipeline {
    converter: MarkdownConverter,      // options mirror beta's html2text knobs
    filter: Option<PruneFilter>,       // None = full markdown; Some = also fit_markdown
}

pub struct ScrapeResult {
    pub url: String,                   // final URL after redirects
    pub status: u16,
    pub markdown: String,              // always bounded
    pub fit_markdown: Option<String>,  // when a filter was requested
    pub raw_html: Option<String>,      // only when explicitly requested, bounded
    pub links: Vec<LinkRecord>,        // capped
    pub metadata: PageMetadata,        // title/description/canonical/og
    pub density: DensityReport,        // bytes-in vs bytes-out per stage
}

pub trait FetchTransport: Send + Sync {          // the ONLY I/O seam
    fn fetch(&self, req: FetchRequest) -> impl Future<Output = Result<FetchResponse, FetchError>>;
}
```

- **Fetch** is a port. The production implementation is sandbox-routed
  (§4.3); tests use recorded fixtures. No HTTP client crate is linked into
  `vesper-web`.
- **Render** is a port (`RenderTransport`), satisfied by the sandboxed
  headless driver (§4.4) when JS execution is required. The pipeline treats
  it as an engine choice, mirroring alpha's waterfall:
  `Fetch` (plain, quality 5, no-JS) → `Render` (headless, quality 20,
  full features). Selection is a fixed two-entry capability table ported
  in spirit from alpha's `buildFallbackList`: try fetch first; on
  JS-signal failures (empty body, status 999-family, content-type lies,
  `noscript`-only bodies) escalate to render once, then fail honestly.
  No stealth, no retries beyond one escalation.
- **Strip** ports alpha's `removeUnwantedElements`: drop `script`, `style`,
  `noscript`, `template`, `svg` (decorative), `iframe` (kept as a labeled
  placeholder in v1), comments, and hidden elements (`display:none`,
  `visibility:hidden`, `aria-hidden`), before any scoring.
- **Prune** (§1.3) and **Convert** (§1.4) produce the two-format output.
- **Extract** emits links and metadata from the stripped DOM, never from
  markdown round-trips.

### 1.3 Prune filters (beta port)

```rust
pub trait PruneFilter: Send + Sync {
    fn filter(&self, dom: &Dom) -> Vec<ContentBlock>; // surviving top-level blocks
}

pub struct TextDensityFilter {          // beta PruningContentFilter
    pub threshold: f32,                 // default 0.48
    pub threshold_type: ThresholdType,  // Fixed | Dynamic
    pub min_word_threshold: Option<usize>,
    pub preserve_classes: HashSet<String>,
    pub preserve_tags: HashSet<String>,
}
```

Metrics and weights ported verbatim from beta's composite score:
`text_density 0.4`, `link_density 0.2` (penalty), `tag_weight 0.2`
(`article 1.5 … span 0.3`), `class_id_weight 0.1` (negative-pattern
regex `nav|footer|header|sidebar|ads|comment|promo|advert|social|share`
ported with the workspace's minimal-features `regex`), `text_length 0.1`.
Dynamic threshold modifiers ported as-is (tag importance >1 → ×0.8;
text ratio >0.4 → ×0.9; link ratio >0.6 → ×1.2). Nodes below threshold
are pruned bottom-up; preserved classes/tags short-circuit scoring.
`min_word_threshold` violations score −1.0 (guaranteed removal).

```rust
pub struct Bm25Filter {                 // beta BM25ContentFilter
    pub threshold: f32,                 // default 1.0
    pub use_stemming: bool,
}
```

Pure Rust BM25 (Okapi) over extracted chunks; query falls back exactly like
beta: user query → title + h1 + meta keywords/description → first
significant paragraph. Priority-tag weights (`h1 5.0 … th 1.5`) boost
chunk scores. Stemming uses the workspace's existing `rust-stemmers`
dependency (already approved for `vesper-cognition`) — English-only v1.
No SQLite, no rank_bm25 dependency.

### 1.4 Markdown conversion

Rust-native converter (html2text-family crate approved through the normal
`cargo deny` gates; exact crate chosen by the PR-0 spike) configured with
beta's option surface: `body_width=0`, `ignore_images`,
`single_line_break`, `mark_code`, plus alpha's post-processing steps
(code-fence de-indentation, blank-line collapse). Deterministic output is
a test fixture contract: same input DOM ⇒ byte-identical markdown.

### 1.5 Map (link discovery + ranking)

`web_map(url, {search?, limit?})`: fetch seed (sandboxed) → collect links
(`a[href]`, canonicalized against base) → if `sitemap.xml` is declared in
robots or resolvable at `/sitemap.xml`, merge sitemap URLs (gzip-aware,
sitemap-count-capped like alpha's `SITEMAP_LIMIT`) → dedupe by normalized
URL → rank by the cosine-similarity port of alpha's `map-cosine.ts`
(character-frequency vectors over link text + URL vs the query; identity
ordering when no query) → return capped list with titles where cheaply
available. Pure ranking function, unit-tested against recorded link sets.

### 1.6 Crawl policy (bounded, denial-reason discipline)

```rust
pub struct CrawlPolicy {
    pub max_urls: usize,        // default 20, hard cap 200
    pub max_depth: usize,       // default 2, hard cap 10
    pub wall_clock_budget: Duration,
    pub concurrency: usize,     // default 2, hard cap 4
    pub same_origin_only: bool, // default true
    pub respect_robots: bool,   // default true
}

pub enum CrawlDenial { DepthLimit, RobotsDisallowed, OffOrigin, BudgetExhausted, DuplicateNormalized }
```

BFS frontier with a visited set keyed on normalized URLs (lowercased
scheme/host, fragment stripped, trailing-slash and query-order
canonicalization). Every excluded URL carries a typed denial reason —
alpha's discipline that makes crawls debuggable by the model itself.
Concurrency is bounded by the transport, which owns the parallelism
budget across the sandbox boundary.

### 1.7 Token budgets and injection hygiene

- Every output is capped by `output_budget_bytes` (default 96 KiB,
  hard cap 512 KiB), applied per field after conversion; `DensityReport`
  tells the model what was cut so it can re-scrape narrower.
- Scraped content is untrusted input. Tool results carry a fixed
  provenance header (`[web content from <origin>]`) and the content
  itself is never interpreted as instructions by any Vesper layer; the
  bounded marker approach mirrors alpha's prompt-injection guard intent
  without attempting LLM classification (§1.8).

### 1.8 What F1 explicitly does not do

No JS-execution heuristics beyond the single fetch→render escalation; no
retry ladders; no per-domain engine pinning; no cross-run caching (§6);
no file-format engines; no language detection beyond the English stemmer.

---

## 2. Feature 2 — The Action Engine (Hands)

### 2.1 Problem

Reading pages is not enough; the ReAct loop must *operate* them: click,
type, navigate, scroll, select. Gamma's contribution is the mapping
discipline that makes an LLM a safe browser operator: a compact indexed
view of interactable elements, stable across steps, with sensitive values
never leaving the page.

### 2.2 Snapshot model (CDP DOMSnapshot port)

`vesper-web::action` defines serde types for CDP
`DOMSnapshot.captureSnapshot` results (documents, strings table, node and
layout arrays) and the enrichment pass ported from gamma:

- `REQUIRED_COMPUTED_STYLES`: the exact 10-style allowlist (`display`,
  `visibility`, `opacity`, `overflow`(+x/y), `cursor`, `pointer-events`,
  `position`, `background-color`) — bounded style capture, not full CSS.
- `EnhancedNode`: node/backend ids, tag, attributes, bounds (`DOMRect`),
  visibility, scrollability, clickability, cursor style, paint order,
  shadow-root and iframe linkage, AX role/name.

### 2.3 Interactable map + serializer

```rust
pub struct InteractableMap {
    pub entries: Vec<InteractableEntry>,   // index → node
}
pub struct InteractableEntry {
    pub index: usize,                      // 1-based, model-facing
    pub tag: String, pub role: Option<String>,
    pub text: String,                      // capped (gamma's cap_text_length)
    pub attributes: BTreeMap<String, String>, // redacted (§2.7)
    pub bounds: Option<DOMRect>, pub is_new: bool,
}
```

- `is_interactive` ports gamma's `ClickableElementDetector`: JS click
  listeners (CDP-detected, no DOM mutation), interactive ARIA roles,
  interactive tag/attribute sets, label/span form-control wrappers
  (depth ≤2), search-indicator classes, large iframes (>100px).
- Serialization renders a markdown-ish tree: text and structure for
  context, `[index]<button>Sign in` lines for interactables, hidden-
  element hints below the fold (gamma's scroll-distance hints), capped at
  `output_budget_bytes`.
- Bounding-box containment filtering (0.99 threshold) and paint-order
  occlusion filtering are ported as post-filters over the same data.

### 2.4 Index stability across steps

The selector map is keyed by `(session_id, backend_node_id)` with a cached
previous state, exactly like gamma's serializer: an element that survives
a DOM mutation keeps its index; new elements get fresh indices marked
`is_new`. A stale index (node gone) is a typed `UnknownIndex` tool error,
never a wrong-element action.

### 2.5 Action registry

```rust
pub enum BrowserAction {
    Navigate { url: String },
    Click { index: usize },
    Type { index: usize, text: String, submit: bool },
    Scroll { direction: Up | Down, amount_pages: u8 },
    SelectOption { index: usize, value: String },
    Back, Forward, Reload,
    Screenshot,                     // bounded, optional
    Close,
}
```

Deliberately minimal v1 (gamma's full registry — file upload, drag, tab
management, wait-for — is phased behind PR-4 evidence, §6). Every action
returns the refreshed interactable map so the loop always sees current
state.

### 2.6 Driver and CDP containment

The browser is a **headless-shell/Chromium process spawned inside the
sandbox** (§4.4), driven over CDP **on `--remote-debugging-pipe`** (fd
3/4), never a TCP port — no inbound surface exists in either host. The
`BrowserDriverPort` (composition boundary) translates `BrowserAction`s
into CDP calls (`Input.dispatchMouseEvent`/`insertText`, `Page.navigate`,
DOM node resolution against the selector map). `vesper-web` itself links
no CDP client crate; it owns only the pure mapping/serialization and the
serde snapshot types, tested against recorded JSON.

### 2.7 Sensitive-value hygiene

Ported exactly: input values for `password`/`file`/`hidden` types and
`cc-*`/`one-time-code` autocomplete never enter the serialized map or any
log; typed secrets echo as `•` masks in tool results. The map exposes
*that* a field exists, never its value.

### 2.8 What F2 explicitly does not do

No captcha solving (fail honestly), no download management, no
multi-tab orchestration, no cookie injection, no network interception
mocking, no vision-based grounding (v1 is DOM-only).

---

## 3. Feature 3 — Tool Definitions

### 3.1 Surfaces

| Tool | Class | Mode | Description |
|---|---|---|---|
| `web_fetch` | Network | Code/Bypass/Ask | one URL → bounded markdown/HTML (no crawl) |
| `web_scrape` | Network | Code/Bypass/Ask | one URL → formats: `markdown`, `fit`, `rawHtml`, `links` (+ render escalation) |
| `web_map` | Network | Code/Bypass/Ask | site link graph, ranked, capped |
| `web_crawl` | Network | Code/Bypass/Ask | bounded multi-URL crawl with per-URL denial reasons |
| `web_interact` | Network | Code/Bypass/Ask | open/reuse a browser session; act by index; returns refreshed map |

A new `ToolExecutionClass::Network` variant (additive; `vesper-domain`
enum + `permission.rs` match arms + mode-eligibility tests updated in the
same PR). Network egress is an external side effect: these tools require
`Code` mode, `Bypass`, or a host-approved `Ask` — they are never
`ReadOnly`, and `Plan` mode excludes them like `Shell`.

### 3.2 Opt-in registration

- `[web]` config section (default absent ⇒ nothing registered):
  `enabled`, `engine.fetch.enabled`, `engine.render.enabled`,
  `interact.enabled` (separate, stricter gate), `respect_robots`,
  `user_agent`, `output_budget_bytes`, `allowlist` (scheme/host/port
  patterns), `deny_private_addresses` (default true), sandbox profile
  (`requirement = "network"`, `allow_network = true` when enabled).
- Tools are registered through `vesper-harness`'s `ToolService` (hosted
  tools, not `parity_default`) with `defer_loading = true` initially:
  hidden from the initial advertisement, surfaced on demand — the
  existing deferred-loading seam, so model context pays nothing until the
  task needs the web.
- **Host parity (binding):** TUI and ACP compose the identical
  `WebService`; a cross-host registration test asserts byte-identical
  definitions and the same sandbox route instance, mirroring the
  VRO-13 firewall/sandbox parity tests.

### 3.3 Failure contract

Every failure is model-facing and recoverable: typed refusal when the
sandbox backend is unavailable (mirroring `route.refusal_text()`), honest
`timed_out`, `budget_exceeded`, `robots_disallowed(url)`, `denied:
private-address`, `render-escalation-failed`. No silent partial output
without a `DensityReport` line stating what was truncated.

---

## 4. Feature 4 — Sandbox Integration

### 4.1 Principle

Untrusted web execution is physically contained. Every byte of network
I/O performed by any web tool flows through a `SandboxRoute` demanding
`IsolationRequirement::Network` with an explicit egress grant
(`SandboxDemand { requirement: Network, allow_network: true, cpu_limit,
memory_limit_bytes }`). The executor-side fail-closed gate
(`route.satisfies_demand()`) runs **before** provisioning: an unreachable
backend yields the model-facing refusal, never an unsandboxed fallback.
There is no configuration in which a web tool performs network I/O
outside the sandbox.

### 4.2 Route construction

Both hosts build the web route from `[web]` config through the same
`vesper-config` sandbox parser used by `[sandbox]` (VRO-13 PR-4 seam),
provisioning either the Linux namespaces backend (feature `linux`) or the
Docker backend (feature `docker`) with `--cpus`/`--memory`/`--pids-limit`
bounds. The route is shared per host process; `SandboxRoute::instance_id`
makes single-instance sharing machine-checkable in the parity test.

### 4.3 Fetch path (no browser)

The sandboxed run executes a small fetch shim (Rust helper binary,
`vesper-web-fetch`, built in-workspace) inside the sandbox: resolve →
robots check (cached per origin in-memory) → allowlist + private-address
denial → single redirect-capped (≤5) fetch with streaming size cap,
content-type sniffing, and charset decoding → bounded body out through
the sandbox's 64 KiB-per-stream output cap, chunked by the driver into
the tool's `output_budget_bytes`. DNS and TLS execute only inside the
sandbox netns.

### 4.4 Render/interact path (headless driver)

A pinned OCI image (built from an in-repo Dockerfile shipping
headless-shell plus the fetch shim; digest pinned in `[web].driver.image`)
runs detached with resource limits and the network grant. The driver:

1. launches headless-shell with `--remote-debugging-pipe` (no TCP),
   `--no-sandbox`, `--disable-gpu`, hardened flags;
2. performs navigation/fetch inside the sandbox netns;
3. streams CDP snapshot/action traffic over the pipe to the host-side
   `BrowserDriverPort`, which feeds `vesper-web`'s pure mapping code;
4. dies with the sandbox (`--rm`, PDEATHSIG chaining on the namespaces
   path) — no orphan processes, ever.

Capability probing is honest: `docker version` cold-start guard (existing
pattern), image-present check, and a smoke navigation; any failure
reports the capability `Unavailable` and the tool fails closed.

### 4.5 Egress policy

v1: egress is granted inside the sandbox instance (page fetches require
it); the **decision** layer is still enforced in pure code before any
navigation — scheme allowlist (`http`/`https`), host allowlist patterns,
private/loopback/link-local address denial (resolved), robots.txt
respect (default on; `robots_disallowed` is a typed denial, not a
warning). Full in-sandbox allowlist *enforcement* (forward proxy) is an
open question (§6), honestly out of v1.

### 4.6 Failure modes

Sandbox unavailable → typed refusal text (existing shape). Daemon/image
missing → capability refusal, never assumed. Wall-clock timeout →
supervisor kill, `timed_out: true`. Output overflow → truncated with an
explicit truncation marker. Driver crash → session invalidated, indices
discarded, model told to re-open.

---

## 5. Verification & Phasing

### 5.1 Per-PR gates (every PR, no exceptions)

1. `cargo test --workspace --all-features` — count ≥ 1,420 and ≥
   predecessor PR's count; no test deleted or weakened.
2. `cargo xtask verify` (fmt + clippy `-D warnings` + tests + architecture).
3. `cargo xtask architecture` — new crate in the allowlist with legal
   dependency direction.
4. `cargo deny check` — advisories/sources/licenses fail-closed for any
   new dependency.
5. Naming-rule grep over the diff (§5.5) returns zero matches.
6. Evidence note `docs/foundation/vro14-prN-<slug>.md` with the test-count
   line and fixture references.

### 5.2 Phase table

| PR | Scope | Lands |
|---|---|---|
| PR-0 | Spike `spikes/web-oracle-portability/`: converter-crate selection, html5ever DOM ergonomics, beta-parity density measurements on ≥10 sample pages | spike verdict + final dep list |
| PR-1 | `crates/vesper-web` skeleton + architecture allowlist + parse→strip→prune(TextDensity)→convert + offline fixtures | F1 core, unit tests |
| PR-2 | `Bm25Filter`, `LinkExtractor`, `MetadataExtractor`, map ranking, `CrawlPolicy`/`CrawlSession` (transport faked) | F1 complete |
| PR-3 | Sandboxed fetch: `WebSandboxPort`, fetch shim, robots, allowlist/private-address denial, budgets; feature-gated integration tests with honest skips | F4 fetch path |
| PR-4 | Action Engine: snapshot serde, interactable map, serializer, stability cache, redaction, `BrowserDriverPort`, pinned image, pipe-CDP driver; recorded-JSON + gated live tests | F2 |
| PR-5 | Tool wiring: `ToolExecutionClass::Network`, `[web]` config, `vesper-harness` `WebService`, defer_loading, host-parity registration tests | F3 |
| PR-6 | Adversarial fixtures + perf gates + `docs/migration-status.md` + `docs/foundation/vro14-final-audit.md` | closeout |

Phases merge strictly in order; each is independently green and
shippable; none touches the ReAct loop internals.

### 5.3 Fixture corpus (`fixtures/web-oracle/`, offline, deterministic)

1. **Content pages** with heavy nav/aside/ads boilerplate → prune density
   assertions (fit-markdown ≤ X% of full, key content retained).
2. **Link-graph pages** → map ranking and crawl denial-reason matrices.
3. **Recorded CDP snapshots** (gamma-derived, redacted) → interactable
   map, index stability across a mutation, sensitive-value redaction.
4. **Adversarial**: 100k-node DOM (perf bound), malformed HTML, shadow
   DOM, cross-origin iframes, meta-refresh chains, content-type lies,
   robots variants, huge attributes, empty bodies.
5. **Converter goldens**: byte-identical markdown fixtures pinning
   converter determinism.

No live network in any default test; live/gated tests are `#[ignore]`d
with honest skip messages, mirroring the existing live-test pattern.

### 5.4 Performance gates

`#[ignore]`d perf tests, run in the release profile and recorded in PR
evidence: prune+convert over the 100k-node fixture < 150 ms; interactable
serialization over a 5k-interactable snapshot < 50 ms; zero allocation on
the no-`[web]`-config path (structural test, mirroring VRO-13's
zero-cost opt-in proof).

### 5.5 Docs & naming-rule obligations

- `crates/AGENTS.md` child index gains `vesper-web/AGENTS.md`;
  `docs/AGENTS.md` PRD list already names this file; migration status
  updated at PR-6.
- Naming-rule grep (run per PR over the diff and at closeout): case-
  insensitive grep for the six upstream brand tokens and their common
  separators/spellings over every artifact this work creates — must
  return zero matches. The exact alternation lives in the closeout
  command, never spelled out in-tree. Upstream citations use the
  alpha/beta/gamma names and the `<pkg-root>` placeholder convention
  (§0); brand names must not appear in PRD, AGENTS.md, README, or
  source comments under any casing.

---

## 6. Open Questions & Honest Divergences

1. **Gamma's fork point.** The pinned gamma revision uses CDP DOMSnapshot
   enrichment, not the classic JS-injection DOM builder of earlier
   releases; this PRD ports the snapshot architecture and says so,
   rather than describing the older design.
2. **Headless browser is not Rust.** The renderer is a pinned headless-
   shell binary/image, not a Rust engine. Vesper's code owns routing,
   containment, mapping, serialization, and policy — the honest boundary,
   documented here rather than hidden.
3. **In-sandbox egress allowlist enforcement** (forward proxy inside the
   netns) is deferred; v1 enforces policy pre-navigation in pure code.
4. **Cross-run content cache** (bounded, under the state root) is
   deferred; every scrape is live in v1.
5. **File engines (PDF/DOCX)** and gamma's fuller action registry
   (uploads, tabs, waits) are explicit follow-ups gated on demand.
6. **English-only stemming** in `Bm25Filter` v1 (existing approved
   dependency); multi-language is a follow-up.
