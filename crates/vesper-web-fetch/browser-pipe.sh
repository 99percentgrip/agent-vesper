#!/bin/sh
set -eu
# Shell fd duplication keeps raw syscalls out of the Rust library. Browser
# stdout/stderr cannot contaminate the NUL-framed CDP channel on fd 4.
exec /usr/bin/chromium-headless-shell \
  --remote-debugging-pipe --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --disable-background-networking \
  --disable-component-update --disable-sync --no-first-run \
  --no-default-browser-check --disable-extensions --disable-breakpad \
  --user-data-dir=/tmp/vesper-browser-profile --window-size=1280,800 \
  about:blank 3<&0 4>&1 0</dev/null 1>/dev/null 2>/dev/null
