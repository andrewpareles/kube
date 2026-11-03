export const getRedirectToUrl = (nextURL: string | undefined) => `${process.env.NEXT_PUBLIC_SITE_URL!}${getSafeNextPath(nextURL)}`

const getSafeNextPath = (nextURL: string | undefined) => {
    if (nextURL === undefined) return '/'
    const siteUrl = new URL(process.env.NEXT_PUBLIC_SITE_URL!)
    const nextUrl = new URL(nextURL, siteUrl)
    if (nextUrl.origin !== siteUrl.origin) return '/'
    return `${nextUrl.pathname}${nextUrl.search}`
}

// The window name of the Google sign in popup, and the name of the BroadcastChannel it tells the page that opened it on.
export const LOGIN_POPUP_NAME = 'login-popup'

// The oauth callback has this param when it is running in that popup.
export const LOGIN_POPUP_SEARCH_PARAM = 'popup'
