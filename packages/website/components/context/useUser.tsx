'use client'

import { createContext, use, useEffect } from "react"
import { useRouter } from "next/navigation"
import { User } from "@supabase/supabase-js"
import posthog from "posthog-js"
import { supabaseOnClient } from "@/supabase/supabaseOnClient"
import { type SiteAccount } from "@/lib/siteAccount"

type UserContextValue = {
    user: User | null
    account: SiteAccount | null
}

// undefined should never be reached inside the provider
const UserContext = createContext<UserContextValue | undefined>(undefined)

const useUserContext = () => {
    const userContext = use(UserContext)
    if (userContext === undefined) {
        throw new Error('useUser must be used within a UserProvider')
    }
    return userContext
}

export const useUser = () => useUserContext().user

export const usePlayerName = () => useUserContext().account?.username ?? null

export const usePlayerUserId = () => useUserContext().account?.userId ?? null

export const usePlayerLook = () => useUserContext().account?.look ?? null

// The user and account are whatever the server rendered the page for. When the browser's own Supabase
// client sees a different user, as when signing out or signing in or out in another tab, the server is
// asked to render again, rather than the page guessing at an account it was never sent.
export const UserProvider = ({ children, user, account }: { children: React.ReactNode, user: User | null, account: SiteAccount | null }) => {
    const router = useRouter()
    const profileId = user === null ? null : user.id
    const userId = account === null ? null : account.userId

    useEffect(() => {
        const supabase = supabaseOnClient()
        const { data: { subscription } } = supabase.auth.onAuthStateChange((_event, session) => {
            if ((session === null ? null : session.user.id) !== profileId) router.refresh()
        })

        return () => subscription.unsubscribe()
    }, [profileId, router])

    useEffect(() => {
        if (userId !== null) posthog.identify(String(userId))
    }, [userId])

    return <UserContext.Provider value={{ user, account }}>{children}</UserContext.Provider>
}
