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

installed_version="$("$install_dir/agent-vesper-acp" --version 2>&1)"
tui_version="$("$install_dir/agent-vesper-tui" --version 2>&1)"
printf 'Installed Agent Vesper (%s; %s):\n' "$installed_version" "$tui_version"
printf '  %s\n' "$install_dir/agent-vesper-acp"
printf '  %s\n' "$install_dir/agent-vesper-tui"

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

# Agent Vesper resolves Z.ai credentials from the environment (no on-disk
# credential store, matching its no-filesystem-I/O contract).
printf '\nNext:\n'
printf '  export ZAI_API_KEY="<your Z.ai key>"   # https://z.ai/\n'
printf '  agent-vesper-acp --setup              # optional private credential store\n'
printf '  agent-vesper-tui                      # launch the terminal harness\n'
printf '\nThen register Agent Vesper as an ACP agent in Zed (see README "Install in Zed").\n'
