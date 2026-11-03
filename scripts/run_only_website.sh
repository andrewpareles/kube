#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT/packages/website"

# Skip the predev renderer build and run only the website.
exec npm run --ignore-scripts dev -- "$@"
