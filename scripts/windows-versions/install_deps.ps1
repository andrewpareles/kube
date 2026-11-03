#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Install the same dependencies and generated bundles as scripts/install_deps.sh.
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$packageDirs = @(
    "packages/kube-ts-api"
    "packages/play-webview"
    "crates/editor_server/node"
    "packages/website"
)
$bundlePackageDirs = @(
    "packages/kube-ts-api"
    "packages/play-webview"
)

Push-Location -LiteralPath $repoRoot
try {
    # Install the JavaScript dependencies in every local package the Rust code depends on.
    foreach ($packageDir in $packageDirs) {
        Write-Host "Installing npm dependencies in $packageDir"
        Set-Location -LiteralPath (Join-Path $repoRoot $packageDir)
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed in $packageDir with exit code $LASTEXITCODE."
        }
    }

    # Rebuild the generated bundles that the Rust client includes directly from disk.
    foreach ($packageDir in $bundlePackageDirs) {
        Write-Host "Bundling $packageDir"
        Set-Location -LiteralPath (Join-Path $repoRoot $packageDir)
        & npm.cmd run bundle
        if ($LASTEXITCODE -ne 0) {
            throw "npm run bundle failed in $packageDir with exit code $LASTEXITCODE."
        }
    }

    # Build the shared Rust renderer for the website's WebGPU preview.
    Set-Location -LiteralPath $repoRoot
    & rustup target add wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) {
        throw "Installing the Rust WebAssembly target failed with exit code $LASTEXITCODE."
    }
    & cargo install wasm-bindgen-cli --version 0.2.114 --locked
    if ($LASTEXITCODE -ne 0) {
        throw "Installing wasm-bindgen-cli failed with exit code $LASTEXITCODE."
    }
    & (Join-Path $PSScriptRoot "build_website_wasm.ps1")
} finally {
    Pop-Location
}
