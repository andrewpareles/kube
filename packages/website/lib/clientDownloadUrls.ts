export const CLIENT_DOWNLOAD_PAGE_PATH = '/download'

export type DetectedOS = {
    platform: 'macos' | 'windows'
    architecture: 'arm64' | 'x64'
}

export type ClientDownload = DetectedOS & {
    fileName: string
    href: string
}

export type DownloadPageRow = {
    label: string
    href: string | null
}

export const CLIENT_DOWNLOADS: ClientDownload[] = [
    {
        platform: 'macos',
        architecture: 'arm64',
        fileName: 'Kube-new_arm64.dmg',
        href: 'https://github.com/andrewpareles/kube/releases/latest/download/Kube-new_arm64.dmg',
    },
    {
        platform: 'windows',
        architecture: 'x64',
        fileName: 'Kube-new_x64.exe',
        href: 'https://github.com/andrewpareles/kube/releases/latest/download/Kube-new_x64.exe',
    },
]

type NavigatorWithUserAgentData = Navigator & {
    userAgentData?: {
        platform: string
        getHighEntropyValues?: (hints: string[]) => Promise<{
            architecture?: string
            bitness?: string
            platform?: string
        }>
    }
}

export const getDownloadUrlForOS = ({ platform, architecture }: DetectedOS): ClientDownload | null => {
    for (const clientDownload of CLIENT_DOWNLOADS) {
        if (clientDownload.platform === platform && clientDownload.architecture === architecture) {
            return clientDownload
        }
    }
    return null
}

export const getAutoDetectedDownloadUrl = async (): Promise<string | null> => {
    if (typeof window === 'undefined') {
        return null
    }

    const browserNavigator: NavigatorWithUserAgentData = window.navigator
    const detectedOS = await detectOSFromClientHints(browserNavigator) ?? detectOSFromNavigator(browserNavigator)
    if (detectedOS === null) {
        return null
    }
    return getDownloadUrlForOS(detectedOS)?.href ?? null
}

const detectOSFromClientHints = async (browserNavigator: NavigatorWithUserAgentData): Promise<DetectedOS | null> => {
    const userAgentData = browserNavigator.userAgentData
    if (!userAgentData?.getHighEntropyValues) {
        return null
    }

    let highEntropyValues: {
        architecture?: string
        bitness?: string
        platform?: string
    }
    try {
        highEntropyValues = await userAgentData.getHighEntropyValues(['architecture', 'bitness', 'platform'])
    } catch {
        return null
    }

    const platformText = `${highEntropyValues.platform ?? userAgentData.platform}`.toLowerCase()
    const platform = detectPlatform(platformText)
    if (platform === null) {
        return null
    }

    const normalizedArchitecture = (highEntropyValues.architecture ?? '').toLowerCase()
    if (normalizedArchitecture === 'arm' && highEntropyValues.bitness === '64') {
        return {
            platform,
            architecture: 'arm64',
        }
    }
    if (normalizedArchitecture === 'x86' && highEntropyValues.bitness === '64') {
        return {
            platform,
            architecture: 'x64',
        }
    }
    return null
}

const detectOSFromNavigator = (browserNavigator: NavigatorWithUserAgentData): DetectedOS | null => {
    const platformText = `${browserNavigator.userAgentData?.platform ?? ''} ${browserNavigator.platform} ${browserNavigator.userAgent}`.toLowerCase()
    const platform = detectPlatform(platformText)
    if (platform === null) {
        return null
    }
    if (platform === 'macos' && window.navigator.maxTouchPoints > 1) {
        return null
    }

    const architecture = platform === 'macos'
        ? detectMacArchitectureFromWebGl() ?? detectArchitectureFromNavigatorText(platformText)
        : detectArchitectureFromNavigatorText(platformText)
    if (architecture === null) {
        return null
    }
    return { platform, architecture }
}

const detectPlatform = (platformText: string): DetectedOS['platform'] | null => {
    if (platformText.includes('mac')) {
        return 'macos'
    }
    if (platformText.includes('win')) {
        return 'windows'
    }
    return null
}

const detectArchitectureFromNavigatorText = (platformText: string): DetectedOS['architecture'] | null => {
    if (platformText.includes('arm64') || platformText.includes('aarch64')) {
        return 'arm64'
    }
    if (
        platformText.includes('win64')
        || platformText.includes('wow64')
        || platformText.includes('x86_64')
        || platformText.includes('amd64')
        || platformText.includes('x64')
    ) {
        return 'x64'
    }
    return null
}

const detectMacArchitectureFromWebGl = (): DetectedOS['architecture'] | null => {
    const canvas = document.createElement('canvas')
    const renderingContext = canvas.getContext('webgl')
    if (!renderingContext) {
        return null
    }

    const debugRendererInfo = renderingContext.getExtension('WEBGL_debug_renderer_info')
    if (debugRendererInfo === null) {
        return null
    }

    const renderer = renderingContext.getParameter(debugRendererInfo.UNMASKED_RENDERER_WEBGL)
    if (typeof renderer !== 'string') {
        return null
    }

    const normalizedRenderer = renderer.toLowerCase()
    if (normalizedRenderer.includes('software')) {
        return null
    }
    if (normalizedRenderer.includes('apple')) {
        return 'arm64'
    }
    if (
        normalizedRenderer.includes('intel')
        || normalizedRenderer.includes('amd')
        || normalizedRenderer.includes('radeon')
        || normalizedRenderer.includes('nvidia')
    ) {
        return 'x64'
    }
    return null
}
