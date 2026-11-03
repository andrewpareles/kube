import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

const readRepoFile = (path: string): string => fs.readFileSync(repoPath(path), "utf8");

test("product branding is consistent across the website and desktop client", () => {
  const websiteLayoutSource = readRepoFile("packages/website/app/layout.tsx");
  const siteMetadataSource = readRepoFile("packages/website/lib/siteMetadata.ts");
  const clientAppRunnerSource = readRepoFile("crates/client/src/app_runner.rs");
  const macosPackagerSource = readRepoFile("packaging/client/Packager.macos.toml");
  const windowsPackagerSource = readRepoFile("packaging/client/Packager.windows.toml");

  // The website writes the name in lower case wherever a person reads it, and the desktop client
  // keeps the capitalised form for its own window, menus and packaging.
  assert.match(siteMetadataSource, /export const SITE_NAME = 'kube\.new'/);
  assert.match(websiteLayoutSource, /default: SITE_NAME,/);
  assert.match(clientAppRunnerSource, /with_title\("Kube\.new"\)/);
  for (const packagerSource of [macosPackagerSource, windowsPackagerSource]) {
    assert.match(packagerSource, /^name = "kube-new"$/m);
    assert.match(packagerSource, /^product-name = "Kube\.new"$/m);
    assert.match(packagerSource, /^identifier = "new\.kube\.client"\s*$/m);
    assert.match(packagerSource, /^schemes = \["kube-new"\]$/m);
  }
});

test("product URLs use Kube.new while machine-facing names use kube-new", () => {
  const websiteEnvSource = readRepoFile("packages/website/.env");
  const deepLinkSource = readRepoFile("packages/website/app/deeplink/sendDeepLinks.ts");
  const clientConstsSource = readRepoFile("crates/game_types/src/consts.rs");
  const clientLoggingSource = readRepoFile("crates/shared/src/env/logging.rs");
  const downloadUrlsSource = readRepoFile("packages/website/lib/clientDownloadUrls.ts");
  const releaseInfoSource = readRepoFile("packages/website/lib/getClientReleaseInfo.ts");
  const releaseWorkflowSource = readRepoFile(".github/workflows/release-packaging.yml");
  const edgegapVersionScriptSource = readRepoFile("scripts/configure_edgegap_version.sh");
  const packagingReadmeSource = readRepoFile("README_PACKAGING.md");

  assert.match(websiteEnvSource, /^SITE_URL=https:\/\/kube\.new$/m);
  assert.match(deepLinkSource, /new URL\('kube-new:\/\/'\)/);
  assert.doesNotMatch(deepLinkSource, /kube\.new:\/\//);
  assert.match(clientConstsSource, /DEEP_LINK_SCHEME: &str = "kube-new"/);
  assert.match(clientLoggingSource, /Library\/Logs\/Kube\.new/);
  assert.match(clientLoggingSource, /Kube\.new\/Logs/);
  assert.match(downloadUrlsSource, /Kube-new_arm64\.dmg/);
  assert.match(downloadUrlsSource, /Kube-new_x64\.exe/);
  assert.match(downloadUrlsSource, /github\.com\/andrewpareles\/kube\/releases\/latest\/download/);
  assert.match(releaseInfoSource, /api\.github\.com\/repos\/andrewpareles\/kube\/releases\/latest/);
  assert.match(releaseInfoSource, /Kube-new_arm64\.app\.tar\.gz/);
  assert.match(releaseInfoSource, /Kube-new_x64\.exe/);
  assert.match(releaseWorkflowSource, /Kube-new_arm64\.app\.tar\.gz/);
  assert.match(releaseWorkflowSource, /Kube-new_arm64\.dmg/);
  assert.match(releaseWorkflowSource, /Kube-new_x64\.exe/);
  assert.match(releaseWorkflowSource, /EDGEGAP_REGISTRY_PROJECT: candy-cane-games-yonqr3v74kch/);
  assert.match(releaseWorkflowSource, /EDGEGAP_IMAGE_NAME: kube-new-server/);
  assert.match(edgegapVersionScriptSource, /"docker_image": "\$\{EDGEGAP_REGISTRY_PROJECT\}\/\$\{EDGEGAP_IMAGE_NAME\}"/);
  assert.doesNotMatch(releaseWorkflowSource, /Kube\.new_|kube\.new-(?:api|server)/);
  assert.match(packagingReadmeSource, /kube-new-server:v/);
  assert.doesNotMatch(`${clientConstsSource}\n${downloadUrlsSource}\n${releaseInfoSource}\n${releaseWorkflowSource}`, /glassdevtools\/plastic/);
});

test("local worlds use the Kube.new world extension", () => {
  for (const path of [
    "dev-demo.kube",
    "game-35-latest.kube",
    "game-bird.kube",
    "game-early-baseplate.kube",
    "game-pigball.kube",
  ]) {
    assert.equal(fs.existsSync(repoPath(path)), true, `${path} should exist`);
  }
});
