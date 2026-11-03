// Use this in Server Components and on the Server API.

import { createServerClient } from '@supabase/ssr'
import { cookies } from 'next/headers'
import { getSupabasePublicKeys } from './getSupabasePublicKeys'
import { Database } from './database.types'

// Create a supabase client on the server.
export async function supabaseOnServer() {
    const { supabaseUrl, supabaseAnonKey } = getSupabasePublicKeys()

    const cookieStore = await cookies()
    return createServerClient<Database>(
        supabaseUrl,
        supabaseAnonKey,
        {
            cookies: {
                // give Supabase the cookies from the incoming request.
                getAll() {
                    return cookieStore.getAll()
                },
                // Supabase wants us to set cookiesToSet in the outgoing response.
                setAll(cookiesToSet) {
                    try {
                        for (const { name, value, options } of cookiesToSet) {
                            cookieStore.set(name, value, options) // implicitly adds the cookie on the Response object
                        }
                    } catch {
                        // supabaseOnServer can be called from multiple places: apis, in Server Components, etc. This `catch` statement is for Server Components. Server Components are not allowed to set cookies, so if supabaseOnServer is used in a Server Component, `cookieStore.set()` will throw an error, caught here.
                        // This `catch` statement is normal, and fine to ignore because of middleware. More here: https://supabase.com/docs/guides/auth/server-side/nextjs.
                    }
                },
            },
        })
}
