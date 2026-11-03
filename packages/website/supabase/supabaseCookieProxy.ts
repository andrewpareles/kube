// Your server runs this before each request from client to server as "middleware" to refresh the authentication cookies if they are invalid.
// See here for supabase docs: https://supabase.com/docs/guides/auth/server-side/nextjs

import { createServerClient } from '@supabase/ssr'
import { NextResponse, type NextRequest } from 'next/server'
import { getSupabasePublicKeys } from './getSupabasePublicKeys'
import { Database } from './database.types'


export async function supabaseCookieProxy(request: NextRequest) {
    let supabaseResponse = NextResponse.next({ request, })

    // set up a supabase serverClient just like the other supabase server file, but with a different `setAll` for middleware.
    const { supabaseUrl, supabaseAnonKey } = getSupabasePublicKeys()
    const supabase = createServerClient<Database>(
        supabaseUrl,
        supabaseAnonKey,
        {
            cookies: {
                // give Supabase the cookies from the incoming request.
                getAll() {
                    return request.cookies.getAll()
                },
                // Supabase wants us to set cookiesToSet in the outgoing response.
                setAll(cookiesToSet) {
                    supabaseResponse = NextResponse.next({ request, }) // this doesn't seem necessary, but supabase does it.
                    for (const { name, value, options } of cookiesToSet) {
                        request.cookies.set(name, value) // make the request look like it had the cookie
                        supabaseResponse.cookies.set(name, value, options) // add the cookie to the response so the user updates it
                    }
                },
            },
        }
    )
    // Note from supabase: Do not run code between createServerClient and supabase.auth.getUser(). A simple mistake could make it very hard to debug issues with users being randomly logged out.
    // IMPORTANT: DO NOT REMOVE auth.getUser()
    const { data: { user }, } = await supabase.auth.getUser()


    // TODO: Andrew removed this if check (just makes stale pages redirect to login). 
    // if (
    //     !user

    //     // TODO: these should actually be valid
    //     && !(request.nextUrl.pathname.startsWith('/login') ||
    //         request.nextUrl.pathname.startsWith('/auth') ||
    //         request.nextUrl.pathname.startsWith('/error'))
    // ) {
    //     // no user, potentially respond by redirecting the user to the login page
    //     const url = request.nextUrl.clone()
    //     url.pathname = '/login'
    //     return NextResponse.redirect(url)
    // }


    // this is from the supbase shadcn integration:
    // if (
    //     !user
    //     &&
    //     !(
    //         request.nextUrl.pathname.startsWith('/login') ||
    //         request.nextUrl.pathname.startsWith('/auth') ||
    //         request.nextUrl.pathname.startsWith('/error') // <-- added by andrew
    //     )
    // ) {
    //     // no user, potentially respond by redirecting the user to the login page
    //     const url = request.nextUrl.clone()
    //     url.pathname = '/auth/login'
    //     return NextResponse.redirect(url)
    // }




    // Note from supabase: IMPORTANT: You *must* return the supabaseResponse object as it is. If you're creating a new response object with NextResponse.next() make sure to:
    // 1. Pass the request in it, like so:    `const myNewResponse = NextResponse.next({ request })`
    // 2. Copy over the cookies, like so:     `myNewResponse.cookies.setAll(supabaseResponse.cookies.getAll())`
    // 3. Change the myNewResponse object to fit your needs, but avoid changing the cookies!
    // 4. Finally:                            `return myNewResponse`
    // If this is not done, you may be causing the browser and server to go out of sync and terminate the user's session prematurely!


    return supabaseResponse
}
