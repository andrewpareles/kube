import assert from "node:assert/strict";
import test from "node:test";
import {
  findGithubReleaseAsset,
  getClientUpdaterAssetInfo,
  type GithubReleaseAsset,
} from "../packages/website/lib/getClientReleaseInfo.ts";

test("client updater platforms use exact Kube-new release artifact names", () => {
  assert.deepEqual(getClientUpdaterAssetInfo({ target: "macos", arch: "aarch64" }), {
    assetName: "Kube-new_arm64.app.tar.gz",
    format: "app",
  });
  assert.deepEqual(getClientUpdaterAssetInfo({ target: "windows", arch: "x86_64" }), {
    assetName: "Kube-new_x64.exe",
    format: "nsis",
  });
});

test("client updater ignores similarly suffixed release artifacts", () => {
  const assets: GithubReleaseAsset[] = [
    { name: "Someone-else_arm64.app.tar.gz", browser_download_url: "https://example.com/wrong" },
    { name: "Kube-new_arm64.app.tar.gz", browser_download_url: "https://example.com/right" },
  ];
  const assetInfo = getClientUpdaterAssetInfo({ target: "macos", arch: "aarch64" });
  assert.notEqual(assetInfo, null);
  assert.equal(findGithubReleaseAsset({ assetInfo, assets })?.browser_download_url, "https://example.com/right");
});
