'use server'

import { revalidatePath } from 'next/cache'
import { and, eq } from 'drizzle-orm'
import { z } from 'zod'
import { db, sql } from '@/lib/db'
import { deleteCatalogFile } from '@/lib/s3StorageServer'
import { hasProfanity } from '@/lib/siteProfanityFilter'
import { CATALOG_PATH } from '@/lib/siteNavigation'
import { requireUser } from '@/lib/siteUser'
import { catalogMetadata } from '@/supabase/migrations/schema'

export type CatalogItemSettings = { name: string, description: string | null, isListed: boolean }

const catalogItemSettingsSchema = z.object({
    name: z.string().trim().min(1, 'Give the item a name.').max(100, 'The name can be at most 100 characters.'),
    description: z.string().trim().max(1000, 'The description can be at most 1000 characters.').nullable().transform(description => description || null),
    isListed: z.boolean(),
})

const readSettings = (input: CatalogItemSettings): { settings: CatalogItemSettings } | { error: string } => {
    const parsed = catalogItemSettingsSchema.safeParse(input)
    if (!parsed.success) return { error: parsed.error.issues[0]?.message ?? 'Those settings are not allowed.' }
    if (hasProfanity(parsed.data.name)) return { error: 'That name is not allowed.' }
    if (parsed.data.description !== null && hasProfanity(parsed.data.description)) return { error: 'That description is not allowed.' }
    return { settings: parsed.data }
}

const itemPath = (catalogItemId: number) => `${CATALOG_PATH}/${catalogItemId}`

export const buyCatalogItem = async (catalogItemId: number): Promise<{ isBought: true } | { error: string }> => {
    const user = await requireUser()
    const id = z.number().int().positive().parse(catalogItemId)
    try {
        await sql`select buy_catalog_item(${user.id}::uuid, ${id}::bigint)`
        revalidatePath(itemPath(id))
        return { isBought: true }
    } catch (error) {
        const { code, message } = error as { code?: string, message?: string }
        // buy_catalog_item raises its own refusals, which are written to be shown as they are.
        if (code === 'P0001' && message) return { error: message }
        throw error
    }
}

export const updateCatalogItem = async (input: { catalogItemId: number } & CatalogItemSettings): Promise<{ settings: CatalogItemSettings } | { error: string }> => {
    const user = await requireUser()
    const catalogItemId = z.number().int().positive().parse(input.catalogItemId)
    const result = readSettings(input)
    if ('error' in result) return result
    const updated = await db
        .update(catalogMetadata)
        .set({ ...result.settings, updatedAt: new Date().toISOString() })
        .where(and(eq(catalogMetadata.catalogItemId, catalogItemId), eq(catalogMetadata.creatorId, user.id)))
        .returning({ catalogItemId: catalogMetadata.catalogItemId })
    if (updated.length === 0) return { error: 'Only the creator can change this item.' }
    revalidatePath(CATALOG_PATH)
    revalidatePath(itemPath(catalogItemId))
    return result
}

// Deleting an item takes it from everyone who owns it and off anyone wearing it, with no refund. Its file goes after its row, and
// the item is gone even if that fails, so the failure is only logged.
export const deleteCatalogItem = async (catalogItemId: number): Promise<{ isDeleted: true } | { error: string }> => {
    const user = await requireUser()
    const id = z.number().int().positive().parse(catalogItemId)
    const deleted = await db
        .delete(catalogMetadata)
        .where(and(eq(catalogMetadata.catalogItemId, id), eq(catalogMetadata.creatorId, user.id)))
        .returning({ catalogItemId: catalogMetadata.catalogItemId })
    if (deleted.length === 0) return { error: 'Only the creator can delete this item.' }
    await deleteCatalogFile(id).catch(error => console.error(`Could not delete the file of catalog item ${id}`, error))
    revalidatePath(CATALOG_PATH)
    revalidatePath(itemPath(id))
    return { isDeleted: true }
}
