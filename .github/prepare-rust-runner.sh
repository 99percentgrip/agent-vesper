#!/bin/sh
set -eu

# This helper is limited to disposable GitHub-hosted Linux machines.
if [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ] || [ "${RUNNER_OS:-}" != Linux ] || [ "$#" -ne 0 ]; then
    echo 'Rust runner preparation requires a GitHub-hosted Linux runner and no arguments.' >&2
    exit 2
fi

df -h / "$GITHUB_WORKSPACE"
for rust_ci_unused_sdk in /usr/local/lib/android /usr/share/dotnet /opt/ghc; do
    if [ -e "$rust_ci_unused_sdk" ]; then
        du -sh "$rust_ci_unused_sdk"
        sudo rm -rf --one-file-system -- "$rust_ci_unused_sdk"
    fi
done
df -h / "$GITHUB_WORKSPACE"
