#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"

package_dirs=(
  "packages/kube-ts-api"
  "packages/play-webview"
  "crates/editor_server/node"
  "packages/website"
)

bundle_package_dirs=(
  "packages/kube-ts-api"
  "packages/play-webview"
)

# This installs the JavaScript dependencies in every local package the Rust code depends on.
for package_dir in "${package_dirs[@]}"; do
  echo "Installing npm dependencies in ${package_dir}"
  (
    cd "${repo_root}/${package_dir}"
    npm ci
  )
done

# This rebuilds the generated bundles that the Rust client includes directly from disk.
for package_dir in "${bundle_package_dirs[@]}"; do
  echo "Bundling ${package_dir}"
  (
    cd "${repo_root}/${package_dir}"
    npm run bundle
  )
done

# Build the shared Rust renderer for the website's WebGPU preview.
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.114 --locked
"${repo_root}/scripts/build_website_wasm.sh"
