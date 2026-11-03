// social login callback page

// This is the page that Google Login and other Auth Servers redirect the user to after the user authenticates on their website. It's part of the standard OAuth flow.
import { NextResponse } from "next/server"
import { supabaseOnServer } from "@/supabase/supabaseOnServer"
import { LOGIN_POPUP_NAME, LOGIN_POPUP_SEARCH_PARAM } from "@/app/login/loginRedirect"
import { welcomeIfNewAccount } from "@/lib/welcomeEmail"

// This page is an OAuth callback handler. Supabase uses it in redirectTo:

// return the user to an error page with instructions
const redirectToErrorPage = ({ origin }: { origin: string }) => {
    return NextResponse.redirect(`${origin}/auth/auth-code-error?error=${encodeURIComponent('Sorry, we had a problem verifying your account. Please try again or contact us for help.')}`)
}

// A popup with no code is one they cancelled on Google's page, so it only closes.
const replyToPopup = ({ isSignedIn }: { isSignedIn: boolean }) => new Response(
    `<script>${isSignedIn ? `new BroadcastChannel('${LOGIN_POPUP_NAME}').postMessage(null);` : ''}window.close()</script>`,
    { headers: { 'Content-Type': 'text/html' } },
)

// This is a route but it acts like a page, and we direct the user's browser to it.
export async function GET(request: Request) {
    const { searchParams, origin } = new URL(request.url)
    const isPopup = searchParams.has(LOGIN_POPUP_SEARCH_PARAM)


    // if "next" is in param, use it as the redirect URL, and if "next" is not a relative URL, use the default. next must be a relative path.
    let next = searchParams.get('next') ?? '/'
    if (!next.startsWith('/')) { next = '/' }

    // PKCE code exchange
    const code = searchParams.get('code')
    if (!code) return isPopup ? replyToPopup({ isSignedIn: false }) : redirectToErrorPage({ origin })

    const supabase = await supabaseOnServer()
    const { data, error } = await supabase.auth.exchangeCodeForSession(code)
    if (error) return redirectToErrorPage({ origin })
    welcomeIfNewAccount(data.user)
    if (isPopup) return replyToPopup({ isSignedIn: true })


    // if local, x-forwarded-host is not relevant
    const isLocalEnv = process.env.NODE_ENV === 'development'
    if (isLocalEnv) {
        return NextResponse.redirect(`${origin}${next}`)
    }


    // get original origin before load balancer
    const forwardedHost = request.headers.get('x-forwarded-host')
    if (forwardedHost) {
        return NextResponse.redirect(`https://${forwardedHost}${next}`)
    }

    return NextResponse.redirect(`${origin}${next}`)

}

