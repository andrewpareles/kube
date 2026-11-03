This is a recap of how we package the binaries for our desktop app client, the game servers, and the editor servers. 

Primarily written by AI.

<details>
  
# Packaging

## Overview

This repo has four packaging or deployment targets, but only three of them live in `packaging/`.

| Target | Local command | Main output | Publish destination | Notes |
| --- | --- | --- | --- | --- |
| `client` | `./scripts/package_macos_client.sh` or `.\scripts\windows-versions\package_windows_client.ps1` | Desktop installer artifacts in `dist/` | GitHub Releases | Installable desktop app from `crates/client`. |
| `editor-server` | `docker build --file packaging/editor-server/Dockerfile ...` | Combined Rust/Node Docker image | Separate Edgegap editor application | Browser editor runtime from `crates/editor_server`; see its [deployment instructions](crates/editor_server/README.md#deploy). |
| `server` | `docker build --file packaging/server/Dockerfile ...` | Local Docker image `kube-new-server:vX.Y.Z` | Edgegap via `.github/workflows/release-packaging.yml` | Game server runtime from `crates/server`. |
| `website` | Outside this folder | Hosting platform output | Website host | Owns client download and update routes plus Edgegap control-plane routes. |

The two runtime stacks are:

- browser editing: `website` + `editor-server`
- play: `client` + `server`

The Next.js routes in `packages/website/app/api/` are part of the `website` target. They handle client download and update metadata plus Edgegap webhooks.

## Directory layout

```text
packaging/
  client/
    Packager.macos.toml
    Packager.windows.toml
    entitlements.plist
    azure_metadata.json
    assets/
    generated-icons/
  server/
    Dockerfile
  editor-server/
    Dockerfile
```

`packaging/client/` holds desktop installer config, signing files, the checked-in source art, and the generated platform icons. The source icon it builds those from lives with the script that draws it, at `scripts/icon-gen/icon-source.png`. `assets/macos/` and `assets/windows/` hold the art the installers are built from, including the checked-in Windows `icon.ico` the generator writes. `packaging/server/` holds the game server Docker build target. `packages/website/` is part of the release story, but it is deployed outside this folder.

The client still uses [`cargo-packager`](https://docs.crabnebula.dev/packager). The server uses a Docker build.

All client installer artifacts are written to `dist/`.

## One-time setup

Install packager for the desktop client:

```bash
cargo install cargo-packager --locked
cargo install tauri-cli --locked
```

`tauri-cli` is only used to cut the macOS icons; the Windows icons are checked in.

Install Docker for local `server` image builds.

Windows only: install NSIS so `makensis` is available on `PATH` for client packaging.

## Package locally

### Client macOS

```bash
./scripts/package_macos_client.sh
```

Equivalent commands:

```bash
rm -rf packaging/client/generated-icons
cargo tauri icon scripts/icon-gen/icon-source.png --output packaging/client/generated-icons
cargo packager --release --config packaging/client/Packager.macos.toml
```

### Client Windows (PowerShell)

```powershell
.\scripts\windows-versions\package_windows_client.ps1
```

Equivalent commands:

```powershell
cargo build -p client --release
cargo packager --release --config packaging/client/Packager.windows.toml
```

### Server Docker image

```bash
docker build --file packaging/server/Dockerfile --tag kube-new-server:v$(node scripts/get_release_version.mjs) .
```

## Important values to customize

- `identifier` in both client packager config files (currently `new.kube.client`)
- `publisher` in the Windows config, which must match the name on the signing certificate
- `product-name` if you want a different app name in installers
- `version` if you want to override crate version explicitly

## Icons

Every icon comes out of one generator, `scripts/icon-gen/generateKubeIcons.mjs`. Do not edit its
outputs by hand — redraw them all, desktop and website, with:

```bash
npm run generate-icons --prefix packages/website
```

**macOS** uses the source image it writes to `scripts/icon-gen/icon-source.png`, a rounded white
tile. `cargo tauri icon` cuts the `.icns` from it into `packaging/client/generated-icons/`, which the
local script and the release workflow both do before packaging. That directory stays uncommitted.

```bash
cargo tauri icon scripts/icon-gen/icon-source.png --output packaging/client/generated-icons
```

**Windows** uses a finished, checked-in `packaging/client/assets/windows/icon.ico`: the mark with no
tile behind it, since nothing on Windows crops an app icon. It holds every size from 16 to 256
pixels, each drawn at its size rather than downsampled, which is what keeps the K readable in a task
bar. Nothing generates it at package time, so there is no icon step in the Windows script or CI job.
Two things read it:

- `crates/client/build.rs` compiles it into `client.exe`, along with `ProductName`,
  `FileDescription`, `CompanyName` and `LegalCopyright`. Windows has no app bundle, so Explorer, the
  task bar, shortcuts and Add/Remove Programs read the icon and the name out of the executable
  itself, and cargo-packager never touches the binary.
- `packaging/client/Packager.windows.toml` points `icons` and the NSIS `installer-icon` at it.

These are not generated and should stay checked in:

- `packaging/client/assets/macos/macos-dmg-background.jpg`
- `packaging/client/assets/windows/nsis-header.bmp`
- `packaging/client/assets/windows/nsis-sidebar.bmp`

## macOS signing + notarization (cargo-packager built-in)

This repo now uses cargo-packager's built-in signing and notarization flow.

### Step 1: Set signing identity in config

In `packaging/client/Packager.macos.toml`, set:

```toml
[macos]
signing-identity = "Developer ID Application: YOUR NAME (TEAMID)"
```

Optional:

```toml
[macos]
entitlements = "entitlements.plist"
```

Check available signing identities with:

```bash
security find-identity -v -p codesigning
```

### Step 2: Provide notarization credentials (choose one mode)

Mode 1: keychain profile (recommended)

Store once:

```bash
xcrun notarytool store-credentials "AC_NOTARY" \
  --apple-id "you@example.com" \
  --team-id "TEAMID" \
  --password "app-specific-password"
```

Use:

```bash
export APPLE_KEYCHAIN_PROFILE="AC_NOTARY"
```

Mode 2: Apple ID credentials directly

```bash
export APPLE_ID="you@example.com"
export APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx"
export APPLE_TEAM_ID="TEAMID"
```

Mode 3: App Store Connect API key directly

```bash
export APPLE_API_KEY="ABC123XYZ"
export APPLE_API_ISSUER="00000000-0000-0000-0000-000000000000"
export APPLE_API_KEY_PATH="/absolute/path/AuthKey_ABC123XYZ.p8"
```

If `APPLE_API_KEY_PATH` is not set, cargo-packager will search common
`private_keys` locations for `AuthKey_<APPLE_API_KEY>.p8`.

### Step 3: Run packager normally

```bash
./scripts/package_macos_client.sh
```

or:

```bash
cargo packager --release --config packaging/client/Packager.macos.toml
```

### CI-only certificate import variables (optional locally)

For CI, cargo-packager can import a base64-encoded `.p12` certificate via:

```bash
export APPLE_CERTIFICATE="...base64-of-p12..."
export APPLE_CERTIFICATE_PASSWORD="p12-password"
```

### Behavior note

cargo-packager's built-in notarization flow notarizes and staples the `.app`
bundle workflow it generates. If you need "submit DMG itself to notarytool",
that is a separate custom flow.

## Windows signing (cargo-packager built-in)

Configure signing in `packaging/client/Packager.windows.toml` under `[windows]`:

- `certificate-thumbprint`: your cert SHA-1 thumbprint
- `digest-algorithm`: usually `sha256`
- `timestamp-url`: RFC3161/Authenticode timestamp URL
- `tsp`: set `true` for RFC3161 (`/tr` + `/td`)
- `sign-command`: optional custom signing command instead of defaults

When these are set, cargo-packager uses built-in Windows signing via `signtool`.
You still need the signing certificate installed and `signtool` available.

In this repo, CI Windows signing is done by `azure/artifact-signing-action@v1`. Local Windows packaging still expects your Azure signing tools setup through `scripts/windows-versions/package_windows_client.ps1`, which prepares `signtool.exe` and the local `Azure.CodeSigning.Dlib.dll`.

## Installer images

### macOS DMG visuals

Use the `[dmg]` section in `packaging/client/Packager.macos.toml`:

- `background`
- `app-position`
- `app-folder-position`
- `window-size`

### Windows NSIS visuals

Use the `[nsis]` section in `packaging/client/Packager.windows.toml`:

- `installer-icon`
- `header-image` (recommended `150x57` bitmap)
- `sidebar-image`

## CI packaging + release (GitHub Actions)

This repo now includes `.github/workflows/release-packaging.yml`.

Trigger:

- Push a tag matching `v*`.
- Example: `git tag vX.Y.Z && git push origin vX.Y.Z`

Pipeline behavior:

- `build-macos` job (macOS):
  - Runs `cargo packager --release --config packaging/client/Packager.macos.toml`
  - Uses cargo-packager native signing + notarization flow for macOS
  - Uploads `.dmg` artifact
  - Signs and uploads the macOS updater bundle `.app.tar.gz` plus `.sig`
- `build-windows` job (Windows):
  - Builds `client` in release mode
  - Signs `target\\release\\client.exe` via `azure/artifact-signing-action@v1`
  - Runs `cargo packager --release --config packaging/client/Packager.windows.toml`
  - Signs `dist\\*.exe` via `azure/artifact-signing-action@v1`
  - Signs and uploads the updater `.exe.sig`
  - Uploads `.exe` installer artifact
- `publish-server-edgegap` job (Linux):
  - Builds `packaging/server/Dockerfile`
  - Pushes the image to the Edgegap registry as `kube-new-server:<release-tag>`
  - Points the existing `prod` Edgegap application version at the `kube-new-server` image repository and release tag
- `release` job:
  - Creates/updates GitHub Release for the pushed tag
  - Uploads updater assets `Kube-new_arm64.app.tar.gz`, `Kube-new_arm64.app.tar.gz.sig`, and `Kube-new_x64.exe.sig`
  - Uploads the static workflow artifact `Kube-new_arm64.dmg`
  - Uploads the static workflow artifact `Kube-new_x64.exe`
  - Uploads static release asset names so website downloads can use GitHub's latest-release asset URLs


### Important macOS native-flow note

For macOS, this workflow intentionally uses cargo-packager native behavior only.

- `.app` is signed, notarized, and stapled in cargo-packager's built-in flow.
- `.dmg` is signed in cargo-packager's built-in flow.
- We do not run custom manual `notarytool submit` for `.dmg` in CI.

## What you must configure

### 1) GitHub repository secrets

In GitHub: `Settings` -> `Secrets and variables` -> `Actions`, add:

Windows / Azure:

- `AZURE_CLIENT_ID`
- `AZURE_TENANT_ID`
- `ARTIFACT_SIGNING_ENDPOINT` (example: `https://eus.codesigning.azure.net/`)
- `ARTIFACT_SIGNING_ACCOUNT`
- `ARTIFACT_SIGNING_PROFILE`

macOS / Apple:

- `APPLE_CERTIFICATE` (base64 encoded `.p12`)
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_API_KEY`
- `APPLE_API_ISSUER`
- `APPLE_API_KEY_P8_BASE64` (base64 encoded `AuthKey_<APPLE_API_KEY>.p8`)

Edgegap:

- `EDGEGAP_API_TOKEN`
- `EDGEGAP_REGISTRY_USERNAME`
- `EDGEGAP_REGISTRY_TOKEN`

There are some more env vars hard-coded in `release-packaging.yml` that are not secrets.

Updater signing:

- `CARGO_PACKAGER_SIGN_PRIVATE_KEY`
- `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD`

### 2) Azure setup (one-time)

You need an Entra app identity for GitHub Actions and Artifact Signing permissions.

Ours already exists, in Mathew's directory, and both founders can reach it. Andrew is a guest there
with Owner on the subscription, both signing roles on the account, and ownership of the app
registration, so either of you can fix a broken release without the other.

Log in to that directory by name:

```bash
az login --tenant "$AZURE_TENANT_ID"
```

`AZURE_TENANT_ID` is in Infisical, in Production `/github`.

| what | value |
| --- | --- |
| App registration | `github-artifact-signing-ci` (its appId is `AZURE_CLIENT_ID` in Infisical) |
| Signing account | `glass-signing-prod` in `glass-resource-group-prod` (eastus) |
| Certificate profile | `glass-certificate`, PublicTrust |

Andrew is a guest in that tenant, so his username there isn't his email. Role assignments have to
name him by object id — `--assignee` with his email fails even though he is a member of the directory.

The certificate profile is PublicTrust, which means Microsoft verified Glass Devtools as a legal
entity to issue it. Do not move this subscription between tenants casually: it is unclear whether
that identity validation survives the move, and redoing it takes days during which no Windows build
can be signed.

- Create or choose a Microsoft Entra app registration.
- Create a federated credential on that app registration:
  - Issuer: `https://token.actions.githubusercontent.com`
  - Subject: this repo + ref pattern you want to allow (usually your release tags)
  - Audience: `api://AzureADTokenExchange`
  - Take the subject verbatim from the `AADSTS700213` error a failed run prints, rather than
    writing out what it ought to be. GitHub now includes numeric owner and repo ids in it, so the
    tidy-looking `repo:owner/name:environment:release` will not match what is actually presented.
    Renaming the repo changes this subject and breaks signing until a new credential is added; that
    is what took v0.1.63 two attempts.
- In Azure, assign that app/service principal the `Artifact Signing Certificate Profile Signer` role.
- Confirm your Artifact Signing account/profile exist and match:
  - `ARTIFACT_SIGNING_ENDPOINT`
  - `ARTIFACT_SIGNING_ACCOUNT`
  - `ARTIFACT_SIGNING_PROFILE`
- Ensure resource provider `Microsoft.CodeSigning` is registered on your subscription.

### 3) Apple setup (one-time)

- Make sure your `packaging/client/Packager.macos.toml` `signing-identity` matches your Developer ID Application certificate subject.
- Export your Developer ID Application certificate to `.p12`.
- Base64 encode that `.p12` and store as `APPLE_CERTIFICATE`.
- Store the `.p12` password as `APPLE_CERTIFICATE_PASSWORD`.
- In App Store Connect, create/get API key credentials:
  - Key ID -> `APPLE_API_KEY`
  - Issuer ID -> `APPLE_API_ISSUER`
  - Download `AuthKey_<KEY_ID>.p8`
- Base64 encode `AuthKey_<KEY_ID>.p8` and store as `APPLE_API_KEY_P8_BASE64`.
- Make sure the Apple Account Holder has accepted the current Apple agreements before you run the macOS release job. If a required agreement is missing or expired, `notarytool` will fail with HTTP 403 during notarization.

### 4) GitHub Actions permissions

The workflow is already configured with required permissions:

- Windows signing job: `id-token: write` (required for Azure OIDC login)
- Release job: `contents: write` (required to create/upload release assets)

### 5) Tagging process for releases

Run:

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

This is the release trigger and is the standard flow used here. Make sure the tag matches the version in the root `Cargo.toml` `[workspace.package]`.

## Local Windows signing behavior update

CI and local packaging both use `packaging/client/Packager.windows.toml`.

CI signing is handled by `azure/artifact-signing-action@v1`.
Local Windows packaging still uses the Azure signing setup prepared by `scripts/windows-versions/package_windows_client.ps1`, including the local `signtool.exe` lookup, `Azure.CodeSigning.Dlib.dll`, and `packaging/client/azure_metadata.json`.

## Apple secrets required in CI

For the current workflow (`.github/workflows/release-packaging.yml`) on GitHub-hosted runners, you should provide all of these:

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_API_KEY`
- `APPLE_API_ISSUER`
- `APPLE_API_KEY_P8_BASE64`

Why all five are needed:

- `APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD` are used so cargo-packager can import your Developer ID certificate into a temporary keychain on the CI runner for code signing.
- `APPLE_API_KEY` + `APPLE_API_ISSUER` + `APPLE_API_KEY_P8_BASE64` are used for notarization auth through App Store Connect API key mode.

### Optional-only cases

- If you use a self-hosted mac runner that already has the correct signing identity installed in keychain, `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` can be omitted.
- If you switch notarization auth mode to Apple ID credentials, replace the API key trio with:
  - `APPLE_ID`
  - `APPLE_PASSWORD`
  - `APPLE_TEAM_ID`

## Azure OIDC environment match

The Windows job is configured with:

- `environment: release`

Your Microsoft Entra federated credential must match that environment subject.


</details>