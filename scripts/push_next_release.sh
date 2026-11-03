#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

release_version="$(node scripts/get_release_version.mjs)"
if [[ -z "$release_version" ]]; then
    echo "Failed to read release version from Cargo.toml." >&2
    exit 1
fi

release_tag="v${release_version}"

if git show-ref --verify --quiet "refs/tags/${release_tag}"; then
    echo "Local tag '$release_tag' already exists." >&2
    exit 1
fi

git fetch --tags origin

if [[ -n "$(git ls-remote --tags origin "refs/tags/${release_tag}")" ]]; then
    echo "Remote tag '$release_tag' already exists." >&2
    exit 1
fi

echo "Release version: $release_version"
echo "Creating and pushing: $release_tag"

git tag -a "$release_tag" -m "$release_tag"
git push origin "$release_tag"
