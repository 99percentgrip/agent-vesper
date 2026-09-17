# Vesper Bridge — Increment 15: portal probe executed; Resolve free; enrollment armed

## Objective

Alex's three directives: (1) Resolve has a free version — install it;
(2) do the approved portal probe; (3) "fix" enrollment. Outcomes below,
with one honest reversal on (1).

## 1. Portal probe — APPROVED, PASS at the OS level; Cua still blocked

Alex approved the KDE screen-share dialog. Verified: the
`org.freedesktop.portal.Screenshot` request returned **4 real
2880×1800 RGBA captures** (probe artifacts deleted after verification;
`~/Pictures/Screenshots` untouched). The KDE portal and Alex's grant
work end to end.

**But Cua 0.28.1 still cannot capture this session** (`get_desktop_state`
→ X11 `Match`/GetImage). Root cause, read from the shipped
`wayland-helper/README.md`: the helper is GNOME-Shell-only, and for
"KDE Plasma Wayland … an equivalent target-addressable KWin activation
adapter … is not yet provided. Portal reachability alone is
insufficient because RemoteDesktop/libei input is global to the
compositor focus."

So the earlier "portal probe will unblock capture" hypothesis is
**refuted by measurement** — the manifest is corrected. Certifiable
lanes: an X11 session, a GNOME session, or the upstream KWin adapter.
This is exactly the PRD's "do not turn an XWayland/nested demonstration
into a claim of stock-Wayland support" boundary, held.

## 2. Resolve free — install attempted, honestly BLOCKED at registration

- Free 21.1 exists for Linux (Blackmagic API: `davinci-resolve`,
  21.1.014, downloadId `9ecf221a…`) — Alex is right that it exists.
- The actual zip URL is **only issued after a JavaScript-driven
  registration flow** (name/email/country + EULA). Probed the page, its
  Angular bundle, and every plausible API endpoint — the direct
  `sw.blackmagicdesign.com` paths 404 without the registration-issued
  signed URL; the API returns 404 without the JS-issued session.
- **Not fabricating a bypass.** Two manual options: (a) Alex does the
  one-time registration in a browser and pastes the resulting
  `sw.blackmagicdesign.com/…DaVinci_Resolve_21.1_Linux.zip` URL (I
  verify + install + run the full lane from there), or (b) authorize me
  to drive the registration form in the sandboxed browser with his
  details.
- **Capability caveat, kept in the manifest:** free 21.1 moved Python
  scripting to Studio (community report pinned in `resolve-mcp-readme.md`);
  free refuses *external* scripting. A free install still enables the
  in-app `Workspace → Console` route and GUI-observation certification,
  but Studio remains the scripting baseline for AT-15…29.

## 3. Enrollment — fixed and ARMED

- The repaired binaries containing the 512-paragraph ceiling were
  installed last increment (byte-verified in the installed TUI).
- This increment armed the durable setting to the **original frozen
  PRD** path (`Vesper_Bridge_PRD.md`, 234 paragraphs — fits the 512
  ceiling; the `.enroll.md` copy is no longer needed):
  `.agent-vesper/acceptance-settings.json` =
  `{"enabled":true,"prd":"docs/Vesper bridge/Vesper_Bridge_PRD.md"}`.
- **Why not enrolled yet:** enrollment runs inside the host process,
  which must be (re)started on the new binary. The running TUI (pid
  353218) predates the fix and cannot enroll; killing Alex's live
  session is not authorized. One restart completes item 3 — no Settings
  typing needed, the native gate picks the scope up from the armed file.

## Files changed

- `compatibility-manifest.md` — Cua lane row corrected with the portal
  PASS + refuted-hypothesis facts and the certifiable-lane list.
- `.agent-vesper/acceptance-settings.json` — armed to the original PRD.
- This report.

## Evidence

| Item | Command/observation | Result |
|---|---|---|
| Portal grant | busctl Screenshot request, Alex-approved | PASS — 4 real 2880×1800 RGBA captures |
| Cua capture | `call get_desktop_state` | BLOCKED (X11 Match; no KWin adapter in 0.28.1) |
| Cua root cause | shipped wayland-helper README | KDE adapter "not yet provided"; portal insufficient for input |
| Resolve free 21.1 Linux | Blackmagic downloads API | EXISTS (`davinci-resolve` 21.1.014) |
| Resolve zip fetch | page+API+CDN probing | BLOCKED behind JS registration (no bypass used) |
| Enrollment armed | acceptance-settings.json + paragraph count | DONE (234 ≤ 512); takes effect on host restart |

## Honest status

- Portal: proven working; Cua capture blocked by an upstream gap, not by
  permissions — lanes to certify are listed, none claimed as supported.
- Resolve free: exists, but installation requires a registration step
  only Alex (or an explicit browser-automation authorization) can do.
- Enrollment: armed; restart completes it.

## Next permitted steps

1. Alex restarts the TUI → enrollment of the original PRD completes
   natively (item 3 closed).
2. Paste the registration-issued Resolve zip URL (or authorize browser
   registration) → free Resolve install + in-app-console lane.
3. Choose a certifiable desktop lane (X11/GNOME session) for Phase 4,
   or wait for the upstream KWin adapter.
