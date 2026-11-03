import 'server-only'

import { eq } from 'drizzle-orm'
import { db } from '@/lib/db'
import { userProfiles } from '@/supabase/migrations/schema'

/** The username behind a profile id, or null if nobody has that id. */
export const getProfileUsername = async (userId: number) => {
    const [profile] = await db
        .select({ username: userProfiles.username })
        .from(userProfiles)
        .where(eq(userProfiles.userId, userId))
        .limit(1)

    return profile?.username ?? null
}
