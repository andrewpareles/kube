#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Build the website's WebAssembly packages without requiring Bash on Windows.
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Push-Location -LiteralPath $repoRoot
try {
    # The CLI version must match the wasm-bindgen version pinned in render_web/Cargo.toml and profanity/Cargo.toml.
    $wasmBindgenVersion = & wasm-bindgen --version
    if ($LASTEXITCODE -ne 0 -or $wasmBindgenVersion -ne "wasm-bindgen 0.2.114") {
        throw "Install wasm-bindgen-cli 0.2.114 to build the website WebAssembly packages."
    }
    # The renderer draws in the browser; the profanity filter runs on the server next to the database.
    foreach ($crateName in @("render_web", "profanity")) {
        & cargo build --locked -p $crateName --target wasm32-unknown-unknown --release
        if ($LASTEXITCODE -ne 0) {
            throw "Building $crateName failed with exit code $LASTEXITCODE."
        }
        & wasm-bindgen "target/wasm32-unknown-unknown/release/$crateName.wasm" --target web --out-dir "packages/website/lib/gen/$crateName"
        if ($LASTEXITCODE -ne 0) {
            throw "Generating the $crateName bindings failed with exit code $LASTEXITCODE."
        }
    }
    # Browsers that give WebGPU no adapter load this WebGL2 build of the renderer instead. It is bigger, so only they download it.
    & cargo build --locked -p render_web --features webgl --target wasm32-unknown-unknown --release
    if ($LASTEXITCODE -ne 0) {
        throw "Building the WebGL2 render_web failed with exit code $LASTEXITCODE."
    }
    & wasm-bindgen "target/wasm32-unknown-unknown/release/render_web.wasm" --target web --out-dir "packages/website/lib/gen/render_web_webgl"
    if ($LASTEXITCODE -ne 0) {
        throw "Generating the WebGL2 render_web bindings failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}
