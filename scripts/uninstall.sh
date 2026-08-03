#!/bin/sh
# Agent Vesper uninstaller — removes the exact launcher, bundled ACP binary,
# and PATH entry created by install.sh. Provider credentials are not touched.
set -eu

install_dir="${AGENT_VESPER_INSTALL_DIR:-${XDG_BIN_HOME:-$HOME/.local/bin}}"
bundle_dir="${AGENT_VESPER_BUNDLE_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/agent-vesper}"

fail() {
    printf 'agent-vesper uninstaller: %s\n' "$1" >&2
    exit 1
}

case "$install_dir" in
    ""|/|.) fail "refusing to remove an empty or root install directory" ;;
esac
case "$bundle_dir" in
    ""|/|.) fail "refusing to remove an empty or root bundle directory" ;;
esac

acp_launcher="$install_dir/agent-vesper-acp"
tui_launcher="$install_dir/agent-vesper-tui"
removed_launcher=0
removed_bundle=0

if [ -e "$acp_launcher" ] || [ -L "$acp_launcher" ]; then
    rm -f "$acp_launcher"
    removed_launcher=1
fi
if [ -e "$tui_launcher" ] || [ -L "$tui_launcher" ]; then
    rm -f "$tui_launcher"
    removed_launcher=1
fi

if [ -d "$bundle_dir" ]; then
    rm -rf "$bundle_dir"
    removed_bundle=1
fi

# Remove only the exact marker/path pair written by install.sh. Leave the
# profile itself and all unrelated PATH entries untouched.
if [ -n "${AGENT_VESPER_SHELL_PROFILE:-}" ]; then
    shell_profile="$AGENT_VESPER_SHELL_PROFILE"
else
    case "${SHELL:-}" in
        */zsh) shell_profile="$HOME/.zprofile" ;;
        *) shell_profile="$HOME/.profile" ;;
    esac
fi

if [ -f "$shell_profile" ]; then
    path_line="export PATH=\"$install_dir:\$PATH\""
    temporary_profile="$(mktemp "$shell_profile.agent-vesper.XXXXXX")"
    awk -v path_line="$path_line" '
        $0 == "# Agent Vesper" { pending_marker = 1; next }
        pending_marker {
            pending_marker = 0
            if ($0 == path_line) { next }
            print "# Agent Vesper"
        }
        { print }
        END {
            if (pending_marker) { print "# Agent Vesper" }
        }
    ' "$shell_profile" > "$temporary_profile"
    if cmp -s "$shell_profile" "$temporary_profile"; then
        rm -f "$temporary_profile"
    else
        mv "$temporary_profile" "$shell_profile"
    fi
fi

# Installers use the environment for ZAI_API_KEY and do not own credentials.
printf 'Agent Vesper uninstall complete.\n'
if [ "$removed_launcher" -eq 1 ]; then
    printf '  Removed %s and %s when present\n' "$acp_launcher" "$tui_launcher"
fi
if [ "$removed_bundle" -eq 1 ]; then
    printf '  Removed %s\n' "$bundle_dir"
fi
printf '  Provider credentials were preserved.\n'
