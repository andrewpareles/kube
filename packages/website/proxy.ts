import { NextResponse, type NextRequest } from 'next/server'
import { supabaseCookieProxy } from '@/supabase/supabaseCookieProxy'

// This is run right before every request comes in. It lets you modify the Request and Response objects that are about to be passed to the program (api handler, server component, etc).

// You can modify the Response object you'd like to be passed to the main program. You can modify the Request object minimally, this is also done through the Response object.
export async function proxy(request: NextRequest) {
  // return NextResponse.next()
  const response = await supabaseCookieProxy(request)
  return response
}

export const config = {
  matcher: [
    /*
     * Match all request paths except for the ones starting with:
     * - _next/static (static files)
     * - _next/image (image optimization files)
     * - favicon.ico (favicon file)
     * Feel free to modify this pattern to include more paths.
     * - robots.txt (search engine crawler rules)
     * - sitemap.xml (public page index)
     */
    '/((?!robots\\.txt$|sitemap\\.xml$|_next/static|_next/image|favicon.ico|.*\\.(?:svg|png|jpg|jpeg|gif|webp)$).*)',
  ],
  // runtime: 'nodejs', // default is 'edge'
}
