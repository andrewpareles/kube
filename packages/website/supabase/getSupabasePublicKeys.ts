// This is a common file for both Server Components and Client Components.

export const getSupabasePublicKeys = () => {
    // We must access env vars statically so Next.js can inline them on the client.
    const supabaseUrl = process.env.NEXT_PUBLIC_SUPABASE_URL
    const supabaseAnonKey = process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY

    if (!supabaseUrl) throw new Error('Please make sure the variable NEXT_PUBLIC_SUPABASE_URL is set in `.env`.')
    if (!supabaseAnonKey) throw new Error('Please make sure the variable NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY is set in `.env`.')

    return { supabaseUrl, supabaseAnonKey }
}

