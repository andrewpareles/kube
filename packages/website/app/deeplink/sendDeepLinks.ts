'use client'

// -------------------------------- send deeplink -------------------------------- 

export type BrowserAfterDeepLink = 'went-away' | 'still-here'

// Watch before opening the app so a quick handoff cannot happen before the listeners are ready.
export const watchBrowserAfterDeepLink = (): Promise<BrowserAfterDeepLink> => {
    return new Promise((resolve) => {
        const onPageHide = () => {
            resolve('went-away')
            cleanupListeners()
        }
        const onVisibilityChange = () => {
            if (document.hidden) {
                resolve('went-away')
                cleanupListeners()
            }
        }

        window.addEventListener('pagehide', onPageHide)
        document.addEventListener('visibilitychange', onVisibilityChange)
        const cleanupListeners = () => {
            window.removeEventListener('pagehide', onPageHide)
            document.removeEventListener('visibilitychange', onVisibilityChange)
            window.clearTimeout(timeoutId)
        }

        const timeoutId = window.setTimeout(() => {
            cleanupListeners()
            // The app can take focus while the browser stays visible behind it.
            resolve(document.hidden || !document.hasFocus() ? 'went-away' : 'still-here')
        }, 1800)
    })
}


type DeepLinkPlayModeParams = {
    requestId: string
    playAuth: string
    thumbnailUuid: string | null
    thumbnailGameName: string
}
export const sendDeepLinkPlayMode = ({
    requestId,
    playAuth,
    thumbnailUuid,
    thumbnailGameName,
}: DeepLinkPlayModeParams): Promise<BrowserAfterDeepLink> => {
    const deepLinkUrl = new URL('kube-new://')
    deepLinkUrl.searchParams.set('mode', 'play')
    deepLinkUrl.searchParams.set('request_id', requestId)
    deepLinkUrl.searchParams.set('play_auth', playAuth)
    if (thumbnailUuid !== null) deepLinkUrl.searchParams.set('thumbnail_uuid', thumbnailUuid)
    deepLinkUrl.searchParams.set('thumbnail_game_name', thumbnailGameName)
    const browserAfterDeepLink = watchBrowserAfterDeepLink()
    window.location.assign(deepLinkUrl)
    return browserAfterDeepLink;
}
