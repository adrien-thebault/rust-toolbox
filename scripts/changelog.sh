#!/usr/bin/env bash
# Regenerate CHANGELOG.md from Conventional Commit history via git-cliff.
# Usage:
#   scripts/changelog.sh                # full changelog, all tags + unreleased
#   scripts/changelog.sh --unreleased   # only commits since the last tag
set -euo pipefail

if ! command -v git-cliff >/dev/null 2>&1; then
    echo "git-cliff is not installed. Install it with: cargo install git-cliff" >&2
    exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."
git-cliff --config cliff.toml --output CHANGELOG.md "$@"
