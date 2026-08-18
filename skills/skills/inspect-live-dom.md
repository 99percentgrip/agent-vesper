---
name: inspect-live-dom
description: Read the live rendered DOM/CSS of a local web or Electron app over the Chrome DevTools Protocol.
version: 2.0.0
author: Agent Vesper library (generic CDP rewrite)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [cdp, dom, css, ui-verification, debugging]
    related_skills: [node-inspect-debugger, systematic-debugging, dogfood]
prerequisites:
  commands: [node]
---

# Inspecting the Live DOM over CDP

When a local dev server or Electron app is running with a Chrome DevTools
Protocol port open, you can read the **live rendered DOM** of the window —
computed styles, geometry, which CSS rule actually won, console output —
instead of inferring from source files and being wrong.

The renderer is a Chromium page, so everything DevTools can read, a script
can read.

**This does not replace looking at it.** CDP answers *factual* questions
("what is the computed padding", "did this element render", "which
selector matches"). It cannot tell you whether the result looks good.
Answer facts with CDP; hand aesthetics to the user.

## When to Use

- Verifying a UI change actually took effect in the running app.
- "Why is this element still X?" — find the winning rule before editing.
- Locating a stable selector for a component you are about to change.
- Checking a design token's computed value on a real node.
- Reading renderer console errors the user mentions but cannot copy out.

## Setup

The target app must be started with a debugging port, e.g.
`--remote-debugging-port=9222` (Chromium/Electron flags). Confirm the
endpoint answers:

    curl -s http://127.0.0.1:9222/json/list | jq '.[] | {title, url}'

If nothing listens on 9222, the app was not started with the flag — ask
the user to restart it with the port; do not scan other ports.

## Procedure

1. Pick the target page from `/json/list` and note its `webSocketDebuggerUrl`.
2. Use a small Node script with the `ws` package (or Chrome's own
   `chrome-remote-interface`) to open the WebSocket and issue CDP commands:

   - `DOM.getDocument` + `DOM.querySelector` — node handles.
   - `CSS.getComputedStyleForNode` — computed values.
   - `DOM.getBoxModel` — geometry.
   - `CSS.getMatchedStylesForNode` — every matching rule and cascade winner.
   - `Runtime.evaluate` — read `getComputedStyle()` directly or console.
   - `Log.enable` / `Runtime.consoleAPICalled` — capture renderer logs.

3. Example — winning rule and computed style for a selector:

       const sel = document.querySelector('.target');
       const cs = getComputedStyle(sel);
       ({font: cs.font, padding: cs.padding, color: cs.color});

4. Report facts with the exact values returned. Never round or "fix" them.

## Failure modes

- `/json/list` empty or connection refused: app not running with the
  debugging port — ask for a restart with the flag.
- Selector misses: print `document.querySelectorAll` count for the exact
  string before concluding the element is absent.
- Inline styles shadowing stylesheets show up only in
  `getMatchedStylesForNode` inline entries — check them before blaming CSS.

## Verification

- Each claim cites a CDP response (command + returned value).
- Selector existence confirmed via query count, not absence of errors.
