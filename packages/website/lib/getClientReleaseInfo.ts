const GITHUB_LATEST_RELEASE_URL = "https://api.github.com/repos/andrewpareles/kube/releases/latest";
export const GITHUB_FETCH_REVALIDATE_SECONDS = 60;

export type GithubReleaseAsset = {
  name: string
  browser_download_url: string
}

export type GithubRelease = {
  tag_name: string
  body: string | null
  published_at: string | null
  assets: GithubReleaseAsset[]
}

export type ClientReleaseAssetInfo = {
  assetName: string
  format: string
}

export const normalizeVersion = (version: string) => version.startsWith("v") ? version.slice(1) : version

const versionPartsOfVersion = (version: string) => {
  const versionMatch = normalizeVersion(version).match(/^(\d+)\.(\d+)\.(\d+)$/)
  if (!versionMatch) {
    throw new Error(`Expected a simple semver version, got ${version}.`)
  }

  return [Number(versionMatch[1]), Number(versionMatch[2]), Number(versionMatch[3])]
}

export const isVersionNewer = ({ latestVersion, currentVersion }: { latestVersion: string, currentVersion: string }) => {
  const latestVersionParts = versionPartsOfVersion(latestVersion)
  const currentVersionParts = versionPartsOfVersion(currentVersion)

  for (let versionPartIndex = 0; versionPartIndex < latestVersionParts.length; versionPartIndex += 1) {
    const latestVersionPart = latestVersionParts[versionPartIndex]
    const currentVersionPart = currentVersionParts[versionPartIndex]
    if (latestVersionPart > currentVersionPart) {
      return true
    }
    if (latestVersionPart < currentVersionPart) {
      return false
    }
  }

  return false
}

export const getClientUpdaterAssetInfo = ({ target, arch }: { target: string, arch: string }): ClientReleaseAssetInfo | null => {
  switch (target) {
    case "macos":
      switch (arch) {
        case "aarch64":
          return {
            assetName: "Kube-new_arm64.app.tar.gz",
            format: "app",
          }
        default:
          return null
      }
    case "windows":
      switch (arch) {
        case "x86_64":
          return {
            assetName: "Kube-new_x64.exe",
            format: "nsis",
          }
        default:
          return null
      }
    default:
      return null
  }
}

export const getLatestGithubRelease = async () => {
  const latestReleaseResponse = await fetch(GITHUB_LATEST_RELEASE_URL, {
    headers: {
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
    },
    next: { revalidate: GITHUB_FETCH_REVALIDATE_SECONDS },
  })

  if (!latestReleaseResponse.ok) {
    throw new Error(`GitHub latest release request failed with status ${latestReleaseResponse.status}.`)
  }

  const latestRelease: GithubRelease = await latestReleaseResponse.json()
  return latestRelease
}

export const findGithubReleaseAsset = ({
  assetInfo,
  assets,
}: {
  assetInfo: ClientReleaseAssetInfo
  assets: GithubReleaseAsset[]
}) => {
  return assets.find(({ name }) => name === assetInfo.assetName)
}
