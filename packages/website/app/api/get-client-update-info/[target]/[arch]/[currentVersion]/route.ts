export const runtime = "nodejs";

import {
  GITHUB_FETCH_REVALIDATE_SECONDS,
  findGithubReleaseAsset,
  getClientUpdaterAssetInfo,
  getLatestGithubRelease,
  isVersionNewer,
  normalizeVersion,
} from "@/lib/getClientReleaseInfo";

// This route turns the latest GitHub release into the one-platform JSON shape
// that cargo-packager-updater expects.

export async function GET(
  _request: Request,
  { params }: { params: Promise<{ target: string, arch: string, currentVersion: string }> },
) {
  try {
    const { target, arch, currentVersion } = await params
    const latestRelease = await getLatestGithubRelease()
    const latestVersion = normalizeVersion(latestRelease.tag_name)

    if (!isVersionNewer({ latestVersion, currentVersion })) {
      return new Response(null, { status: 204 })
    }

    const updaterAssetInfo = getClientUpdaterAssetInfo({ target, arch })
    if (!updaterAssetInfo) {
      return new Response(null, { status: 204 })
    }

    // Find the exact updater bundle and the matching signature file for this platform.
    const updaterAsset = findGithubReleaseAsset({ assetInfo: updaterAssetInfo, assets: latestRelease.assets })
    if (!updaterAsset) {
      throw new Error(`Missing updater asset for ${target} ${arch} on the latest release.`)
    }

    const updaterSignatureAsset = latestRelease.assets.find(({ name }) => name === `${updaterAsset.name}.sig`)
    if (!updaterSignatureAsset) {
      throw new Error(`Missing updater signature asset ${updaterAsset.name}.sig on the latest release.`)
    }

    const updaterSignatureResponse = await fetch(updaterSignatureAsset.browser_download_url, {
      next: { revalidate: GITHUB_FETCH_REVALIDATE_SECONDS },
    })
    if (!updaterSignatureResponse.ok) {
      throw new Error(`Failed reading updater signature with status ${updaterSignatureResponse.status}.`)
    }

    const updaterSignature = await updaterSignatureResponse.text()

    return Response.json({
      version: latestVersion,
      url: updaterAsset.browser_download_url,
      signature: updaterSignature,
      format: updaterAssetInfo.format,
      pub_date: latestRelease.published_at ?? undefined,
      notes: latestRelease.body || undefined,
    })
  } catch (error) {
    console.error("Failed to resolve client updater manifest.", error)
    return new Response("Failed to resolve client updater manifest.", { status: 500 })
  }
}
