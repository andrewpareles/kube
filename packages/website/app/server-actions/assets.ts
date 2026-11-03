'use server'

import { revalidatePath } from 'next/cache'
import { and, eq } from 'drizzle-orm'
import { z } from 'zod'
import { db } from '@/lib/db'
import { hasProfanity } from '@/lib/siteProfanityFilter'
import { assetTypes, type AssetType } from '@/lib/assetTypes'
import { ASSETS_PAGE_SIZE, getAssets } from '@/lib/siteAssets'
import { ASSETS_PATH } from '@/lib/siteNavigation'
import { requireUser } from '@/lib/siteUser'
import { assetMetadata } from '@/supabase/migrations/schema'

const assetSearchSchema = z.object({ query: z.string(), assetType: z.enum(assetTypes).optional(), offset: z.number().int().nonnegative().safe() })

// Assets are public, so searching them needs no login.
export const searchAssets = async (search: { query: string, assetType: AssetType | undefined, offset: number }) => {
    const { query, assetType, offset } = assetSearchSchema.parse(search)
    const assets = await getAssets({ query, assetType, offset, limit: ASSETS_PAGE_SIZE + 1 })
    return { items: assets.slice(0, ASSETS_PAGE_SIZE), hasMore: assets.length > ASSETS_PAGE_SIZE }
}

// Deleting an asset takes it out of Assets with every revision. Games that downloaded it keep their copy, since the files it
// points at stay where they are.
export const deleteAsset = async (assetId: number): Promise<{ isDeleted: true } | { error: string }> => {
    const user = await requireUser()
    const id = z.number().int().positive().parse(assetId)
    const deleted = await db
        .delete(assetMetadata)
        .where(and(eq(assetMetadata.assetId, id), eq(assetMetadata.uploadedBy, user.id)))
        .returning({ versionNum: assetMetadata.versionNum })
    if (deleted.length === 0) return { error: 'Only the person who uploaded an asset can delete it.' }
    revalidatePath(ASSETS_PATH)
    revalidatePath(`${ASSETS_PATH}/${id}`)
    return { isDeleted: true }
}

// A new name applies to every revision, so the asset reads the same wherever an older one shows.
export const updateAssetSettings = async ({ assetId, name }: { assetId: number, name: string }): Promise<{ name: string } | { error: string }> => {
    const user = await requireUser()
    const id = z.number().int().positive().parse(assetId)
    const newName = z.string().parse(name).trim()
    if (newName === '') return { error: 'Give the asset a name.' }
    if (hasProfanity(newName)) return { error: 'That name is not allowed.' }
    const updated = await db
        .update(assetMetadata)
        .set({ name: newName })
        .where(and(eq(assetMetadata.assetId, id), eq(assetMetadata.uploadedBy, user.id)))
        .returning({ versionNum: assetMetadata.versionNum })
    if (updated.length === 0) return { error: 'Only the person who uploaded an asset can change it.' }
    revalidatePath(ASSETS_PATH)
    revalidatePath(`${ASSETS_PATH}/${id}`)
    return { name: newName }
}
