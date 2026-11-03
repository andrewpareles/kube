'use client'

import { useEffect } from 'react'
import Script from 'next/script'
import { toast } from 'sonner'
import { supabaseOnClient } from '@/supabase/supabaseOnClient'
import { LOGIN_SEARCH_PARAM } from '@/app/deeplink/deeplinkFlowConsts'
import { welcomeNewAccount } from '@/app/server-actions/welcome-email'
import { getRedirectToUrl } from './loginRedirect'

const showGoogleOneTap = async () => {
    // Static pages like /privacy are drawn for nobody, so the browser's own session is what says they are signed in.
    const supabase = supabaseOnClient()
    const { data: { session } } = await supabase.auth.getSession()
    if (session !== null) return

    // Google puts the nonce's hash in the id token, and Supabase checks that hash against the nonce itself.
    const nonce = btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32))))
    const nonceHash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(nonce))
    const hashedNonce = Array.from(new Uint8Array(nonceHash), (byte) => byte.toString(16).padStart(2, '0')).join('')

    google.accounts.id.initialize({
        client_id: process.env.NEXT_PUBLIC_GOOGLE_CLIENT_ID!,
        nonce: hashedNonce,
        use_fedcm_for_prompt: true,
        // Otherwise Google drops the request on any click on the page, often before Chrome has shown the picker.
        cancel_on_tap_outside: false,
        // Signing in fires onAuthStateChange, and UserProvider then draws the page again for them.
        callback: async ({ credential }) => {
            const { error } = await supabase.auth.signInWithIdToken({ provider: 'google', token: credential, nonce })
            if (error) {
                toast.error('Failed to sign in with Google.')
                return
            }
            void welcomeNewAccount()
            const next = new URLSearchParams(window.location.search).get(LOGIN_SEARCH_PARAM)
            if (next !== null) window.location.assign(getRedirectToUrl(next))
        },
    })
    google.accounts.id.prompt()
}

// Google's account picker, shown by itself on every page while they are signed out, the way Wavedash does it.
export const GoogleOneTap = () => {
    useEffect(() => () => {
        if (typeof google !== 'undefined') google.accounts.id.cancel()
    }, [])

    return <Script src="https://accounts.google.com/gsi/client" onReady={() => void showGoogleOneTap()} />
}
