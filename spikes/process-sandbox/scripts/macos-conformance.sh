#!/bin/sh
set -eu

# Resolve the spike root so the script works regardless of the caller's
# working directory (platform-foundation.yml invokes it from the workspace
# root; foundation-spikes.yml uses working-directory).
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

test "$(uname -s)" = "Darwin"
test -x /usr/bin/sandbox-exec
/usr/bin/sandbox-exec -p '(version 1) (deny default) (allow process*) (allow file-read*)' /usr/bin/true

cargo test --locked --test process_conformance

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT
mkdir "$tmp_root/workspace" "$tmp_root/outside"
profile="$tmp_root/profile.sb"
sed "s|__WORKSPACE__|$tmp_root/workspace|g" scripts/macos-workspace.sb > "$profile"
/usr/bin/sandbox-exec -f "$profile" /bin/sh -c \
  "touch '$tmp_root/workspace/allowed'; ! touch '$tmp_root/outside/denied'"
test -f "$tmp_root/workspace/allowed"
test ! -e "$tmp_root/outside/denied"
