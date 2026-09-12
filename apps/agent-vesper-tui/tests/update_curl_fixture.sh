#!/bin/sh
# Immutable offline curl fixture for the production updater's installer test.
set -eu
asset=
output=
next_output=0
for argument in "$@"; do
    if [ "$next_output" = 1 ]; then
        output=$argument
        next_output=0
        continue
    fi
    case "$argument" in
        -o) next_output=1 ;;
        https://github.com/99percentgrip/agent-vesper/releases/download/v9.8.7/*)
            asset=${argument##*/} ;;
        https://github.com/astral-sh/*) exit 1 ;;
    esac
done
[ -n "$asset" ] && [ -n "$output" ]
cp "$VESPER_UPDATE_FIXTURES/$asset" "$output"
