import 'server-only'

import { cache } from 'react'
import { supabaseOnServer } from '@/supabase/supabaseOnServer'

/**
 * Who is asking, taken from the cookies on the request. Every page data fetcher that changes what
 * it returns based on who is signed in reads the user from here.
 *
 * supabase.auth.getUser() asks the Supabase auth server to revalidate the token each time it is
 * called, so it is a network round trip rather than a cookie read. cache() makes the whole render
 * share one of those, however many things ask for the user.
 */
export const getUser = cache(async () => {
    const supabase = await supabaseOnServer()
    const { data: { user } } = await supabase.auth.getUser()
    return user
})

/**
 * The signed-in user, for the callers that have nothing to do when nobody is signed in. Callers that
 * can carry on signed out, such as a page that shows fewer games, use getUser and check for null.
 */
export const requireUser = async () => {
    const user = await getUser()
    if (user === null) {
        throw new Error('Unauthorized')
    }

    return user
}
