#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# The CLI version must match the wasm-bindgen version pinned in render_web/Cargo.toml and profanity/Cargo.toml.
if [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.114" ]]; then
  echo 'Install wasm-bindgen-cli 0.2.114 to build the website WebAssembly packages.' >&2
  exit 1
fi

# The renderer draws in the browser; the profanity filter runs on the server next to the database.
for crate_name in render_web profanity; do
  cargo build --locked -p "$crate_name" --target wasm32-unknown-unknown --release
  wasm-bindgen "target/wasm32-unknown-unknown/release/$crate_name.wasm" --target web --out-dir "packages/website/lib/gen/$crate_name"
done

# Browsers that give WebGPU no adapter load this WebGL2 build of the renderer instead. It is bigger, so only they download it.
cargo build --locked -p render_web --features webgl --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/render_web.wasm --target web --out-dir packages/website/lib/gen/render_web_webgl
