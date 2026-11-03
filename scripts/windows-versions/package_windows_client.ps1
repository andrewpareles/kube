Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $repoRoot

# bundle js fresh before packaging
& npm --prefix "packages/kube-ts-api" run bundle
& npm --prefix "packages/play-webview" run bundle


if (-not (Get-Command cargo-packager -ErrorAction SilentlyContinue)) {
    throw "cargo-packager is not installed. Install with: cargo install cargo-packager --locked"
}

if (-not (Get-Command makensis.exe -ErrorAction SilentlyContinue)) {
    Write-Warning "makensis.exe was not detected on PATH. cargo-packager may still find its own NSIS binary."
}

# THIS IS LOCAL OBVIOUSLY vvvvvvv
if (-not (Get-Command signtool.exe -ErrorAction SilentlyContinue)) {
    $sdkBinRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
    $signtoolPath = Get-ChildItem -Path $sdkBinRoot -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like "*\\x64\\signtool.exe" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName

    if ($signtoolPath) {
        $signtoolDir = Split-Path -Parent $signtoolPath
        $env:PATH = "$signtoolDir;$env:PATH"
        Write-Host "Using signtool from: $signtoolDir"
    } else {
        throw "signtool.exe was not found. Install Windows SDK Signing Tools."
    }
}
# THIS IS LOCAL OBVIOUSLY ^^^^^^^

$artifactSigningDlibPath = Join-Path $env:LOCALAPPDATA "Microsoft\MicrosoftArtifactSigningClientTools\Azure.CodeSigning.Dlib.dll"
$packagerDlibPath = Join-Path $repoRoot "packaging\client\Azure.CodeSigning.Dlib.dll"

if (Test-Path $artifactSigningDlibPath) {
    Copy-Item -Path $artifactSigningDlibPath -Destination $packagerDlibPath -Force
    Write-Host "Using Artifact Signing dlib from: $artifactSigningDlibPath"
} elseif (-not (Test-Path $packagerDlibPath)) {
    Write-Warning "Azure.CodeSigning.Dlib.dll was not found. Install Artifact Signing Client Tools or copy the dlib to packaging\\client\\Azure.CodeSigning.Dlib.dll"
}

& cargo build -p client --release

$packagerArgs = @(
    "packager"
    "--release"
    "--config"
    "packaging/client/Packager.windows.toml"
)

$packagerArgs += $args

& cargo @packagerArgs
