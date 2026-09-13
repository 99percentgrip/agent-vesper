#!/bin/sh
set -eu
base=$(dirname "$0")
printf '%s\n' "$*" >> "$base/calls"
if [ "${1:-}" = --connection ]; then shift 2; fi
case "${1:-}" in
  info) [ ! -f "$base/unhealthy" ]; cat "$base/os" ;;
  context) cat "$base/endpoint" ;;
  run) [ ! -f "$base/browser-failed" ]; printf '<html><title>vesper-ready</title></html>\n' ;;
  rm) [ ! -f "$base/cleanup-failed" ] ;;
  machine)
    case "${2:-}" in
      list) if [ -f "$base/machine-created" ]; then printf '[{"Name":"agent-vesper"}]'; else printf '[]'; fi ;;
      init) touch "$base/machine-created" ;;
      start) rm -f "$base/unhealthy" ;;
      *) exit 8 ;;
    esac ;;
  ps) [ ! -f "$base/cleanup-failed" ] ;;
  hang) exec sleep 10 ;;
  *) exit 9 ;;
esac
