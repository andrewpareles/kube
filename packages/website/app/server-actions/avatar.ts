'use server'

import { eq } from 'drizzle-orm'
import { z } from 'zod'
import { db } from '@/lib/db'
import { requireUser } from '@/lib/siteUser'
import { userProfiles } from '@/supabase/migrations/schema'

// Saves the color and hat picked under My Avatar together. A null color means none was ever picked, and only a hat the player owns can be
// equipped, which the database checks. Games read both when a player joins, so a player already in a game keeps their old look until they
// join again.
export const saveAvatarLook = async ({ color, catalogItemId }: { color: string | null, catalogItemId: number | null }) => {
    const user = await requireUser()
    const preferredColor = z.string().regex(/^#[0-9a-f]{6}$/).nullable().parse(color)
    const equippedCatalogItemId = z.number().int().positive().nullable().parse(catalogItemId)
    await db.update(userProfiles).set({ preferredColor, equippedCatalogItemId }).where(eq(userProfiles.id, user.id))
}
