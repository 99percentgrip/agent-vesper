#!/bin/sh
# Agent Vesper installer — downloads the compiled ACP and native TUI binaries
# for the host platform, verifies their archive SHA-256, installs them under
# `$XDG_BIN_HOME` (default `~/.local/bin`), and ensures the install directory
# is on `PATH`. Mirrors the original Python `native-glm-acp` installer UX.
set -eu

repository="99percentgrip/agent-vesper"
release_base="${AGENT_VESPER_RELEASE_BASE_URL:-https://github.com/$repository/releases}"
version="${AGENT_VESPER_VERSION:-latest}"
install_dir="${AGENT_VESPER_INSTALL_DIR:-${XDG_BIN_HOME:-$HOME/.local/bin}}"
bundle_dir="${AGENT_VESPER_BUNDLE_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/agent-vesper}"

fail() {
    printf 'agent-vesper installer: %s\n' "$1" >&2
    exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

case "$(uname -s)" in
    Linux) platform="linux" ;;
    Darwin) platform="darwin" ;;
    *) fail "unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
    x86_64|amd64) architecture="x86_64" ;;
    arm64|aarch64) architecture="aarch64" ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
esac

asset="agent-vesper-acp-$platform-$architecture.tar.gz"
if [ "$version" = "latest" ]; then
    download_root="$release_base/latest/download"
else
    case "$version" in
        v*) tag="$version" ;;
        *) tag="v$version" ;;
    esac
    download_root="$release_base/download/$tag"
fi

temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

printf 'Downloading %s...\n' "$asset"
curl -fL --retry 3 --show-error --silent "$download_root/$asset" -o "$temporary/$asset"
curl -fL --retry 3 --show-error --silent "$download_root/$asset.sha256" -o "$temporary/$asset.sha256"

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$temporary" && sha256sum -c "$asset.sha256")
elif command -v shasum >/dev/null 2>&1; then
    expected="$(awk '{print $1}' "$temporary/$asset.sha256")"
    actual="$(shasum -a 256 "$temporary/$asset" | awk '{print $1}')"
    [ "$actual" = "$expected" ] || fail "SHA-256 verification failed"
else
    fail "sha256sum or shasum is required to verify the download"
fi

tar -xzf "$temporary/$asset" -C "$temporary"
[ -f "$temporary/agent-vesper-acp/agent-vesper-acp" ] || \
    fail "archive did not contain agent-vesper-acp bundle"
[ -f "$temporary/agent-vesper-acp/agent-vesper-tui" ] || \
    fail "archive did not contain agent-vesper-tui"

mkdir -p "$install_dir"
mkdir -p "$(dirname "$bundle_dir")"
rm -rf "$bundle_dir"
mv "$temporary/agent-vesper-acp" "$bundle_dir"
printf '#!/bin/sh\nexec "%s/agent-vesper-acp" "$@"\n' "$bundle_dir" > "$install_dir/agent-vesper-acp"
printf '#!/bin/sh\nexec "%s/agent-vesper-tui" "$@"\n' "$bundle_dir" > "$install_dir/agent-vesper-tui"
chmod 0755 "$install_dir/agent-vesper-acp"
chmod 0755 "$install_dir/agent-vesper-tui"

# Seed the curated skill library into the cross-project memory root.
# Never destructive: existing files win (user edits preserved), slugs listed
# in the seed manifest are never resurrected (user deletions preserved), and
# new seed skills added in later releases are seeded on upgrade.
memory_root="${AGENT_VESPER_MEMORY_ROOT:-$HOME/.agent-vesper/memory}"
seed_root="$bundle_dir/skills"
seeded_count=0
if [ -d "$seed_root/skills" ]; then
    mkdir -p "$memory_root/skills" "$memory_root/bundles"
    manifest="$memory_root/.seed-manifest"
    touch "$manifest"
    for seed_md in "$seed_root/skills/"*.md; do
        [ -f "$seed_md" ] || continue
        slug="$(basename "$seed_md" .md)"
        grep -Fqx "$slug" "$manifest" && continue
        [ -e "$memory_root/skills/$slug.md" ] && continue
        cp "$seed_md" "$memory_root/skills/$slug.md"
        if [ -d "$seed_root/skills/$slug" ] && [ ! -d "$memory_root/skills/$slug" ]; then
            cp -R "$seed_root/skills/$slug" "$memory_root/skills/$slug"
        fi
        printf '%s\n' "$slug" >> "$manifest"
        seeded_count=$((seeded_count + 1))
    done
    for seed_bundle in "$seed_root/bundles/"*.json; do
        [ -f "$seed_bundle" ] || continue
        [ -e "$memory_root/bundles/$(basename "$seed_bundle")" ] && continue
        cp "$seed_bundle" "$memory_root/bundles/"
    done
fi
printf 'Seeded %s skill(s) into %s\n' "$seeded_count" "$memory_root"

installed_version="$("$install_dir/agent-vesper-acp" --version 2>&1)"
tui_version="$("$install_dir/agent-vesper-tui" --version 2>&1)"
printf 'Installed Agent Vesper (%s; %s):\n' "$installed_version" "$tui_version"
printf '  %s\n' "$install_dir/agent-vesper-acp"
printf '  %s\n' "$install_dir/agent-vesper-tui"

# Bundle the standalone `uv` binary so the push-to-talk voice backend can
# auto-bootstrap a `faster-whisper` venv with no external toolchain required
# (Linux/macOS only; voice recording is unsupported on Windows). This closes
# the dependency on a pre-installed `uv`/`python3-venv`. A failed download is
# non-fatal: the install still succeeds and voice falls back to system tools.
uv_warn() {
    printf 'agent-vesper installer: warning — uv download failed; voice backend will use system uv/python3-venv if available\n' >&2
}
case "$platform-$architecture" in
    linux-x86_64)   uv_target="x86_64-unknown-linux-gnu" ;;
    linux-aarch64)  uv_target="aarch64-unknown-linux-gnu" ;;
    darwin-x86_64)  uv_target="x86_64-apple-darwin" ;;
    darwin-aarch64) uv_target="aarch64-apple-darwin" ;;
    *) uv_target="" ;;
esac
if [ -n "$uv_target" ]; then
    uv_asset="uv-$uv_target.tar.gz"
    uv_url="https://github.com/astral-sh/uv/releases/latest/download/$uv_asset"
    printf 'Bundling uv for voice backend (%s)...\n' "$uv_target"
    if curl -fL --retry 3 --show-error --silent "$uv_url" -o "$temporary/$uv_asset"; then
        tar -xzf "$temporary/$uv_asset" -C "$temporary"
        # uv release archive layout: uv-<target>/uv and uv-<target>/uvx
        uv_src="$(find "$temporary" -type f -name uv -path "*$uv_target*" 2>/dev/null | head -1)"
        [ -z "$uv_src" ] && uv_src="$(find "$temporary" -type f -name uv 2>/dev/null | head -1)"
        if [ -n "$uv_src" ] && "$uv_src" --version >/dev/null 2>&1; then
            cp "$uv_src" "$bundle_dir/uv"
            chmod 0755 "$bundle_dir/uv"
            printf '  %s/uv (%s)\n' "$bundle_dir" "$("$bundle_dir/uv" --version 2>&1)"
        else
            uv_warn
        fi
    else
        uv_warn
    fi
fi

case ":${PATH:-}:" in
    *":$install_dir:"*) ;;
    *)
        if [ -n "${AGENT_VESPER_SHELL_PROFILE:-}" ]; then
            shell_profile="$AGENT_VESPER_SHELL_PROFILE"
        else
            case "${SHELL:-}" in
                */zsh) shell_profile="$HOME/.zprofile" ;;
                *) shell_profile="$HOME/.profile" ;;
            esac
        fi
        path_line="export PATH=\"$install_dir:\$PATH\""
        if [ ! -f "$shell_profile" ] || ! grep -Fqx "$path_line" "$shell_profile"; then
            printf '\n# Agent Vesper\n%s\n' "$path_line" >> "$shell_profile"
        fi
        printf '\nAdded %s to PATH in %s. Open a new terminal to use it.\n' \
            "$install_dir" "$shell_profile"
        ;;
esac

# Agent Vesper accepts an environment credential, the Agent Vesper
# Authentication screen, or the explicit setup command. Persisted secrets use
# the OS credential manager when available, with the documented private-vault
# fallback.
printf '\nNext:\n'
printf '  agent-vesper-tui                      # launch; authentication screen opens if needed\n'
printf '  agent-vesper-acp --setup              # optional non-interactive setup\n'
printf '  export ZAI_API_KEY="<your Z.ai key>"   # optional environment override\n'
printf '\nThen register Agent Vesper as an ACP agent in Zed (see README "Install in Zed").\n'
