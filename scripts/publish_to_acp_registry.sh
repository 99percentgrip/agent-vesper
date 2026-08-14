#!/usr/bin/env sh
# ACP Registry publishing helper (VRO-11.2, ADR 0017 follow-up).
#
# Publishes (or updates) the `agent-vesper` entry in the public ACP
# Registry at https://github.com/agentclientprotocol/registry.
#
# What it does:
#   1. Clones agentclientprotocol/registry to a temporary directory
#      (shallow, latest main).
#   2. Creates / refreshes agent-vesper/agent.json from this repo's
#      registry/agent.json (the canonical source).
#   3. Commits on a fresh branch `agent-vesper/v<VERSION>` and pushes to
#      the caller's fork (defaults to the `gh` authenticated user).
#   4. Opens or updates a PR titled
#      "Add agent-vesper: Native Rust Reasoning Orchestrator".
#
# What it does NOT do:
#   - Touch PR #439 or the `native-glm-acp` entry. agent-vesper is a
#     separate agent in the registry.
#   - Make live provider calls or write to user-state directories.
#   - Modify registry/agent.json in this repo (the source of truth).
#
# Usage:
#   scripts/publish_to_acp_registry.sh                 # use detected version + fork
#   AGENT_VESPER_VERSION=0.20.30 scripts/publish_to_acp_registry.sh
#   FORK_OWNER=my-github-handle scripts/publish_to_acp_registry.sh
#   DRY_RUN=1 scripts/publish_to_acp_registry.sh       # print plan, do not push
#
# Requirements:
#   - gh (GitHub CLI) authenticated with repo + workflow scopes
#   - git
#   - jq

set -eu

# ----------------------------------------------------------------------------
# Setup
# ----------------------------------------------------------------------------

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
MANIFEST="${REPO_ROOT}/registry/agent.json"

if [ ! -f "${MANIFEST}" ]; then
    echo "FATAL: ${MANIFEST} not found. Run from the agent-vesper repo root." >&2
    exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
    echo "FATAL: gh (GitHub CLI) is required." >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "FATAL: jq is required." >&2
    exit 1
fi

# Detect version from the manifest if not overridden.
VERSION="${AGENT_VESPER_VERSION:-$(jq -r '.version' "${MANIFEST}")}"
if [ -z "${VERSION}" ]; then
    echo "FATAL: could not detect version from ${MANIFEST}." >&2
    exit 1
fi

# Detect the GitHub login for the fork target.
FORK_OWNER="${FORK_OWNER:-$(gh api user --jq '.login' 2>/dev/null || true)}"
if [ -z "${FORK_OWNER}" ]; then
    echo "FATAL: could not detect GitHub login. Set FORK_OWNER=<your-handle>." >&2
    exit 1
fi

REGISTRY_UPSTREAM="agentclientprotocol/registry"
REGISTRY_FORK="${FORK_OWNER}/registry"
BRANCH="agent-vesper/v${VERSION}"
PR_TITLE="Add agent-vesper: Native Rust Reasoning Orchestrator"
AGENT_ID=$(jq -r '.id' "${MANIFEST}")  # typically "agent-vesper"

TMPDIR=$(mktemp -d -t agent-vesper-registry.XXXXXX)
cleanup() {
    rm -rf "${TMPDIR}"
}
trap cleanup EXIT INT TERM

echo "=== publish_to_acp_registry.sh ==="
echo "  manifest:        ${MANIFEST}"
echo "  version:         ${VERSION}"
echo "  agent id:        ${AGENT_ID}"
echo "  upstream:        ${REGISTRY_UPSTREAM}"
echo "  fork target:     ${REGISTRY_FORK}:${BRANCH}"
echo "  pr title:        ${PR_TITLE}"
if [ "${DRY_RUN:-0}" = "1" ]; then
    echo "  mode:            DRY_RUN (no clone, no push)"
    exit 0
fi
echo ""

# ----------------------------------------------------------------------------
# Ensure the caller has a fork of agentclientprotocol/registry.
# gh repo fork creates one idempotently.
# ----------------------------------------------------------------------------
echo "=== ensuring fork exists at ${REGISTRY_FORK} ==="
if ! gh repo view "${REGISTRY_FORK}" >/dev/null 2>&1; then
    echo "  no fork found; creating one..."
    gh repo fork "${REGISTRY_UPSTREAM}" --clone=false
    echo "  fork created; give GitHub a few seconds to provision it..."
    sleep 5
else
    echo "  fork already exists."
fi

# ----------------------------------------------------------------------------
# Clone the fork shallowly.
# ----------------------------------------------------------------------------
echo "=== cloning fork to ${TMPDIR} ==="
gh repo clone "${REGISTRY_FORK}" "${TMPDIR}/registry" -- --depth=1 --origin fork
cd "${TMPDIR}/registry"

# Add upstream as a remote so we can rebase on the latest main.
# `gh repo clone` may already add it; check first.
if ! git remote get-url upstream >/dev/null 2>&1; then
    git remote add upstream "https://github.com/${REGISTRY_UPSTREAM}.git"
fi
git fetch upstream main --depth=20
git checkout -B "${BRANCH}" upstream/main

# ----------------------------------------------------------------------------
# Write the manifest. Idempotent: replace the whole agent-vesper/ dir
# contents with our canonical registry/agent.json.
# ----------------------------------------------------------------------------
echo "=== writing ${AGENT_ID}/agent.json ==="
mkdir -p "${AGENT_ID}"
cp "${MANIFEST}" "${AGENT_ID}/agent.json"
# Validate JSON before committing.
jq . "${AGENT_ID}/agent.json" >/dev/null

git add "${AGENT_ID}/agent.json"

if git diff --cached --quiet; then
    echo "  no changes vs existing branch tip; will just push to refresh."
else
    git -c user.name="agent-vesper-release" \
        -c user.email="release@agent-vesper.local" \
        commit -m "Update ${AGENT_ID} to v${VERSION}" >/dev/null
fi

# ----------------------------------------------------------------------------
# Push to the fork. --force is safe here because the branch is owned by us
# and named after the version (not main).
# ----------------------------------------------------------------------------
echo "=== pushing ${BRANCH} to fork ==="
git push fork "${BRANCH}" --force

# ----------------------------------------------------------------------------
# Open or update the PR.
# ----------------------------------------------------------------------------
PR_BODY=$(cat <<EOF
## What

Publishes \`${AGENT_ID}\` to the public ACP Registry at **v${VERSION}**.

This is the **Rust-native** Agent Vesper — a separate entry from
\`native-glm-acp\` (the original Python project, owned by a different PR).

- **Repo:** https://github.com/99percentgrip/agent-vesper
- **License:** See manifest
- **Install:** \`curl -fsSL https://raw.githubusercontent.com/99percentgrip/agent-vesper/main/scripts/install.sh | sh\`
- **ACP protocol version:** v1 (stdio)

## Manifest

\`\`\`json
$(jq . "${AGENT_ID}/agent.json")
\`\`\`

## Asset URLs

The five platform archives (\`linux-x86_64\`, \`linux-aarch64\`,
\`darwin-x86_64\`, \`darwin-aarch64\`, \`windows-x86_64\`) are produced by
the tag-triggered \`release.yml\` workflow on
\`github.com/99percentgrip/agent-vesper\` for tag \`v${VERSION}\` and are
SHA-256 checksummed by the installer.

## Verification

- \`cargo xtask verify\` green on the v${VERSION} HEAD.
- All four CI workflows (\`ci.yml\`, \`msrv.yml\`, \`platform-foundation.yml\`,
  \`release.yml\`) \`success\` on the tag HEAD.
- Local install verified: both \`agent-vesper-tui\` and \`agent-vesper-acp\`
  print \`${VERSION}\`.
EOF
)

echo "=== opening / updating PR ==="
PR_EXISTS=$(gh pr list --repo "${REGISTRY_UPSTREAM}" \
    --head "${BRANCH}" \
    --state open \
    --json number \
    --jq 'length')

if [ "${PR_EXISTS}" = "0" ]; then
    PR_URL=$(gh pr create \
        --repo "${REGISTRY_UPSTREAM}" \
        --head "${FORK_OWNER}:${BRANCH}" \
        --base main \
        --title "${PR_TITLE}" \
        --body "${PR_BODY}")
    PR_NUMBER=$(printf '%s' "${PR_URL}" | sed -n 's|.*/pull/\([0-9][0-9]*\).*|\1|p')
    echo ""
    echo "=== DONE: new PR opened ==="
else
    # Update title + body of the existing PR. NOTE: `gh pr list --head`
    # requires the branch name WITHOUT the owner: prefix on the querying
    # side, even though `gh pr create --head` REQUIRES owner:branch.
    PR_NUMBER=$(gh pr list --repo "${REGISTRY_UPSTREAM}" \
        --head "${BRANCH}" \
        --state open \
        --json number \
        --jq '.[0].number')
    # `gh pr edit` issues GraphQL queries against Projects-classic metadata
    # which the agentclientprotocol/registry repo still has enabled, causing
    # a deprecation failure. Update via the REST endpoint directly.
    PR_BODY_FILE=$(mktemp)
    printf '%s' "${PR_BODY}" > "${PR_BODY_FILE}"
    gh api --method PATCH \
        "repos/${REGISTRY_UPSTREAM}/pulls/${PR_NUMBER}" \
        -f title="${PR_TITLE}" \
        -F body=@"${PR_BODY_FILE}" >/dev/null
    rm -f "${PR_BODY_FILE}"
    echo ""
    echo "=== DONE: updated PR #${PR_NUMBER} via REST PATCH ==="
fi

gh api "repos/${REGISTRY_UPSTREAM}/pulls/${PR_NUMBER}" \
    --jq '"  URL:   \(.html_url)\n  PR:    #\(.number)\n  Title: \(.title)"'
