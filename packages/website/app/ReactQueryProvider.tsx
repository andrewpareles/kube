'use client'

import React, { useEffect } from 'react'
import { QueryClientProvider, QueryClient } from '@tanstack/react-query'
import { ReactQueryDevtools } from '@tanstack/react-query-devtools'
import { supabaseOnClient } from '@/supabase/supabaseOnClient'

const client = new QueryClient()

export function ReactQueryProvider({ children }: React.PropsWithChildren) {
    // clear react query cache when user signs out
    useEffect(() => {
        const supabase = supabaseOnClient()
        const { data: { subscription } } = supabase.auth.onAuthStateChange((event) => {
            if (event === 'SIGNED_OUT') client.clear()
        })
        return () => subscription.unsubscribe()
    }, [])


    return (
        <QueryClientProvider client={client}>
            {children}
            <ReactQueryDevtools initialIsOpen={false} />
        </QueryClientProvider>
    )
}
