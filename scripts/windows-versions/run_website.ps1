#Requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Start the website from its package directory, wherever this script was called.
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Push-Location -LiteralPath $repoRoot
try {
    & node scripts/run_website.mjs
    if ($LASTEXITCODE -ne 0) {
        throw "Website startup failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}
