#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# bundle js fresh before packaging
npm --prefix packages/kube-ts-api run bundle
npm --prefix packages/play-webview run bundle

if ! command -v cargo-tauri >/dev/null 2>&1; then
    echo "tauri-cli is not installed. Install with: cargo install tauri-cli --locked"
    exit 1
fi

rm -rf packaging/client/generated-icons
cargo tauri icon scripts/icon-gen/icon-source.png --output packaging/client/generated-icons

cargo packager --release --config packaging/client/Packager.macos.toml "$@"
