# VRO-14 closeout

Date: 2026-09-07. Current production acceptance and release prerequisites:
[gap audit](vro14-gap-audit.md). Installation and operation:
[contained web tools](../web-tools.md).

The earlier PR-0–PR-6 module landing was not production completion. The
v0.20.88 repair still lacked a working browser, render escalation, sitemap
discovery and a published pinned driver. Those findings drove the current
production implementation and real-container acceptance; historical phase
reports remain historical evidence, not current capability claims.

Local acceptance now covers 1,672 passing workspace tests on MSRV 1.88,
canonical verification, both unchanged release-profile performance gates,
real pipe-browser actions/redaction/stale-index/deadline handling, and an
explicit real navigation/chunked-fetch check. The full requirement-to-source
and test map lives in the gap audit rather than duplicating drifting counts.

The original PRD non-goals remain unchanged. The ten-style correction follows
the pinned source; parser/converter substrate choices and the actual shared
web route type are disclosed in the audit. No mocked action, unavailable image,
zero-valued identity comparison, or passing pure fixture substitutes for a
production browser acceptance run.

Release readiness additionally requires successful exact-commit canonical,
MSRV, five-target and dual-architecture driver CI before tagging. The release
publishes the already-tested images, never a post-tag untested rebuild.
