#!/bin/sh
# Offline test-only CLI. Invoked through a per-test symlink; state is adjacent
# to that symlink, never in the source tree. The executable itself is immutable.
set -eu
if [ "$1" = image ]; then
    if [ -f "$0.require-load" ] && [ ! -f "$0.loaded" ]; then
        exit 1
    fi
    cat "$0.response"
elif [ "$1" = load ] && [ "$2" = --input ]; then
    printf 'load\n' >> "$0.loaded"
else
    exit 1
fi
