import 'server-only'

import { eq } from 'drizzle-orm'
import { db } from '@/lib/db'
import { getPlayerLook, type PlayerLook } from '@/lib/siteAvatar'
import { userProfiles } from '@/supabase/migrations/schema'

/**
 * What the header shows about whoever is signed in: their name and their avatar.
 */

export type SiteAccount = {
    username: string
    userId: number
    // Drawn as their avatar beside their name.
    look: PlayerLook
}

export const getSiteAccount = async ({ profileId }: { profileId: string }): Promise<SiteAccount | null> => {
    const [row] = await db
        .select({
            userId: userProfiles.userId,
            username: userProfiles.username,
        })
        .from(userProfiles)
        .where(eq(userProfiles.id, profileId))
        .limit(1)

    if (row === undefined) return null

    return {
        username: row.username,
        userId: row.userId,
        look: await getPlayerLook(row.userId),
    }
}
