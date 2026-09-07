# VRO-14 closeout

Status: COMPLETE for v1, released as `v0.20.89` on 2026-09-07.
Verified production, platform, image and public-release evidence:
[gap audit](vro14-gap-audit.md). Installation and operation:
[contained web tools](../web-tools.md).

The earlier PR-0–PR-6 module landing was not production completion. The
v0.20.88 repair still lacked a working browser, render escalation, sitemap
discovery and a published pinned driver. Those findings drove the current
production implementation and real-container acceptance; historical phase
reports remain historical evidence, not current capability claims.

Local acceptance now covers 1,673 passing workspace tests on MSRV 1.88,
canonical verification, both unchanged release-profile performance gates,
real pipe-browser actions/redaction/stale-index/deadline handling, and an
explicit real navigation/chunked-fetch check. The full requirement-to-source
and test map lives in the gap audit rather than duplicating drifting counts.

The original PRD non-goals remain unchanged. The ten-style correction follows
the pinned source; parser/converter substrate choices and the actual shared
web route type are disclosed in the audit. No mocked action, unavailable image,
zero-valued identity comparison, or passing pure fixture substitutes for a
production browser acceptance run.

Exact-commit canonical, MSRV, five-target and dual-architecture driver CI
passed before tagging; all five application builds and public publication
passed afterward. The release contains the already-tested images, not a
post-tag untested rebuild. Registry PR #539 was updated in place and awaits
upstream review.
