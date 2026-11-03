#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

# Vercel installs the npm dependencies, but the browser renderer also needs Rust.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
  -y --profile minimal --default-toolchain stable --target wasm32-unknown-unknown --no-modify-path
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

# The CLI version must match the wasm-bindgen version pinned in render_web/Cargo.toml.
cargo install wasm-bindgen-cli --version 0.2.114 --locked

cd "$repo_root/packages/website"
npm run build
