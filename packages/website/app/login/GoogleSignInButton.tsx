'use client'

import { useEffect } from 'react'
import { FcGoogle } from 'react-icons/fc'
import { supabaseOnClient } from '@/supabase/supabaseOnClient'
import { getRedirectToUrl, LOGIN_POPUP_NAME, LOGIN_POPUP_SEARCH_PARAM } from './loginRedirect'

// TODO: AI-PICKED-VALUE: 500x650, the size Wavedash opens its sign in popup at, which fits Google's account chooser.
const LOGIN_POPUP_WIDTH = 500
const LOGIN_POPUP_HEIGHT = 650

// Google's pages cut the popup off from window.opener, so it reports back over a BroadcastChannel. When the browser
// blocks the popup, the whole page goes to Google instead.
export const GoogleSignInButton = ({ next }: { next: string }) => {
    useEffect(() => {
        const channel = new BroadcastChannel(LOGIN_POPUP_NAME)
        channel.onmessage = () => window.location.assign(getRedirectToUrl(next))
        return () => channel.close()
    }, [next])

    const signInWithGoogle = async () => {
        const left = Math.round(window.screenX + (window.outerWidth - LOGIN_POPUP_WIDTH) / 2)
        const top = Math.round(window.screenY + (window.outerHeight - LOGIN_POPUP_HEIGHT) / 2)
        // Opened before anything is awaited, because browsers only allow a popup that opens right on a click.
        const popup = window.open('', LOGIN_POPUP_NAME, `popup,width=${LOGIN_POPUP_WIDTH},height=${LOGIN_POPUP_HEIGHT},left=${left},top=${top}`)
        const popupParam = popup === null ? '' : `&${LOGIN_POPUP_SEARCH_PARAM}=1`
        const { data } = await supabaseOnClient().auth.signInWithOAuth({
            provider: 'google',
            options: {
                redirectTo: getRedirectToUrl(`/auth/oauth-callback?next=${encodeURIComponent(next)}${popupParam}`),
                skipBrowserRedirect: popup !== null,
            },
        })
        if (popup !== null && data.url !== null) popup.location.href = data.url
    }

    return (
        <button
            type="button"
            onClick={signInWithGoogle}
            className="w-full flex items-center justify-center gap-2.5 py-2.5 px-4 bg-black text-white font-medium rounded-xl hover:bg-black/80 transition-colors"
        >
            <FcGoogle aria-hidden={true} className="size-[18px]" />
            Continue with Google
        </button>
    )
}
