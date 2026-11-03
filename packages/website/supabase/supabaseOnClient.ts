// Use this in Client Components and on the Browser.

import { createBrowserClient } from '@supabase/ssr'
import { getSupabasePublicKeys } from './getSupabasePublicKeys'
import { Database } from './database.types'


// Create a supabase client on the browser/"client".
export function supabaseOnClient() {
  const { supabaseUrl, supabaseAnonKey } = getSupabasePublicKeys()
  return createBrowserClient<Database>(
    supabaseUrl,
    supabaseAnonKey
  )
}
