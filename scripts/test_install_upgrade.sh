#!/bin/sh
# Offline regression for the production POSIX payload replacement block.
set -eu
repository=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
test_root=$(mktemp -d -t vesper-install-test.XXXXXX)
trap 'rm -rf "$test_root"' EXIT HUP INT TERM
export bundle_dir="$test_root/installed"
export temporary="$test_root/download"
mkdir -p "$bundle_dir/cognition" "$bundle_dir/voice-venv" "$temporary/agent-vesper-acp"
printf 'durable memory\n' > "$bundle_dir/cognition/cognition.db"
printf 'user voice environment\n' > "$bundle_dir/voice-venv/state"
printf 'custom user data\n' > "$bundle_dir/custom"
for payload in agent-vesper-acp agent-vesper-tui vesper-web-fetch sandbox_init; do
    printf 'old binary\n' > "$bundle_dir/$payload"
    printf 'new binary\n' > "$temporary/agent-vesper-acp/$payload"
done
for payload in skills web-driver; do
    mkdir -p "$bundle_dir/$payload" "$temporary/agent-vesper-acp/$payload"
    touch "$bundle_dir/$payload/stale" "$temporary/agent-vesper-acp/$payload/current"
done
before=$(ls -i "$bundle_dir/cognition/cognition.db")
# Execute the actual installer block, not a duplicated implementation.
sed -n '/^# The data root also contains/,/^# Older installers/{ /^# Older installers/!p; }' \
    "$repository/scripts/install.sh" | sh -eu
test "$before" = "$(ls -i "$bundle_dir/cognition/cognition.db")"
test "$(cat "$bundle_dir/cognition/cognition.db")" = 'durable memory'
test "$(cat "$bundle_dir/voice-venv/state")" = 'user voice environment'
test "$(cat "$bundle_dir/custom")" = 'custom user data'
for payload in agent-vesper-acp agent-vesper-tui vesper-web-fetch sandbox_init; do
    test "$(cat "$bundle_dir/$payload")" = 'new binary'
done
for payload in skills web-driver; do
    test -f "$bundle_dir/$payload/current"
    test ! -e "$bundle_dir/$payload/stale"
done
printf 'Installer upgrade preserves user state and database inode: PASS\n'
