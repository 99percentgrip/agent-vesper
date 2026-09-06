# vesper-web-fetch

## Purpose

Own the VRO-14 PR-3 sandboxed fetch route: the `vesper-web-fetch` helper
binary (the only unit that performs web I/O, and only ever inside the
ADR-0022 sandbox) plus `WebSandboxPort`, the `FetchTransport`
implementation that provisions a backend demanding
`IsolationRequirement::Network` with an explicit network grant
(`docs/web-oracle-extraction-prd.md`, Feature 4).

## Ownership

- `src/browser.rs` — ephemeral container-backed pipe-CDP sessions with real
  node resolution, action dispatch, AX/snapshot observations, stable indexes,
  bounded deadlines, and document-navigation admission inside the sandbox.
- `Dockerfile` and `browser-pipe.sh` — immutable base/build image pins and
  exact headless package version; fd 3/4 mapping uses a fixed shell script,
  never a TCP debugging listener or a new Rust unsafe boundary.

- `src/main.rs` — the helper binary: redirect-capped (≤5) blocking fetch,
  bounded streaming (64 KiB stdio route, up to 512 KiB chunked pipe route),
  content-type sniffing, charset decoding (UTF-8/16, Latin-1 family),
  body to stdout, one `VWMETA:` JSON status line to stderr. No cookies,
  no credential jars, no proxy env.
- `src/route.rs` — `WebSandboxPort` + `WebSandboxRouteConfig`: pure
  egress gate first (denied URLs never provision anything), fail-closed
  capability check (`network: Available` or refuse), then provision →
  run helper → parse VWMETA → assemble `FetchResponse`.
- `src/parse.rs` — `VWMETA:` line parsing (last line wins, unknown keys
  ignored) and the helper-output → `FetchResponse` assembly with the
  status-code gate.
- `tests/egress_and_route.rs` — the offline egress denial matrix
  (loopback/localhost/private/link-local/metadata ranges, v6 and
  v4-mapped), the fake-backend routing path, and the `#[ignore]d`
  live-docker execution proof (feature-gated behind `docker`, matching
  `vesper-sandbox`'s own gate).

## Local Contracts

- The harness process never performs a web fetch. Every egress byte flows
  through the helper executing inside a provisioned sandbox with an
  explicit `allow_network = true` grant for that request.
- The helper accepts URL, byte cap, and serialized policy; validates every
  resolved address, pins the approved DNS answers, disables proxy inheritance,
  and checks redirects and robots inside the sandbox. Both raw and decoded
  bodies must fit the cap. Redirect counts are exact, not inferred.
- Successful metadata starts with `VWMETA:`; route parsing preserves empty-
  stdout error reasons, uses the last metadata line, and awaits teardown even
  after a failed run. CPU/memory bounds and route timeouts apply per call.
- Requests above the ordinary 64 KiB command-output cap use the sandbox's
  duplex port and helper `--pipe` protocol, with JSON chunks individually
  bounded below 64 KiB and total decoded body bounded at 512 KiB.
- Gzip bodies are bounded both before and after decompression. Browser
  document requests (including redirects) pause for helper-side DNS/robots
  admission before continuation; the full subresource enforcing proxy remains
  outside v1. No page-supplied JavaScript is accepted as an action argument.
- The egress gate (`vesper-web::egress`) runs before provisioning:
  address-class denials outrank scheme policy so a loopback probe over
  plain http reports as `loopback_address`, not `plain_http_disallowed`.
- `reqwest` is permitted only in this crate's helper binary — the xtask
  production-sources scan carries the explicit exception. The library
  target must stay transport-free (it links only the sandbox/security/web
  crates).
- Default builds carry zero new dependencies: the docker backend and its
  tests are behind the `docker` feature, mirroring `vesper-sandbox`.
- `#![forbid(unsafe_code)]` everywhere in this crate.

## Work Guidance

- The VWMETA contract is versioned by the `"vwf":"1"` marker; bump the
  marker when changing the JSON shape and update both `main.rs` emission
  and `parse.rs` in the same change.
- Live-docker proof: `cargo test -p vesper-web-fetch --features docker
  --test egress_and_route -- --ignored` with a reachable daemon and an
  image carrying the helper on PATH.

## Verification

- `cargo test -p vesper-web-fetch`
- `cargo test -p vesper-web-fetch --all-features real_pipe_browser_actions --
  --ignored` with `VESPER_WEB_TEST_IMAGE` and a working container runtime;
  this explicit acceptance gate fails, rather than skips, missing prerequisites.
- `cargo clippy -p vesper-web-fetch --all-targets -- -D warnings`
- `cargo run --package xtask --quiet -- architecture`

## Child DOX Index

No children.
