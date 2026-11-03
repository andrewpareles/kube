import { drizzle } from 'drizzle-orm/postgres-js'
import postgres from 'postgres'
import * as schema from '@/supabase/migrations/schema'

const drizzle_database_url = process.env.DRIZZLE_DATABASE_URL
if (!drizzle_database_url) {
    throw new Error('Missing DRIZZLE_DATABASE_URL in the website runtime environment')
}

// Use the Supabase transaction pooler at runtime so Next workers do not exhaust session-mode connections.
const runtime_database_url = new URL(drizzle_database_url)
runtime_database_url.port = '6543'
export const sql = postgres(runtime_database_url.toString(), {
    prepare: false,
    max: 1,
})
export const db = drizzle({ client: sql, schema })
