import 'server-only'

import { desc, eq } from 'drizzle-orm'
import { db } from '@/lib/db'
import { readAssetFile } from '@/lib/siteAssets'
import { readCatalogFile } from '@/lib/siteCatalog'
import { assetMetadata, catalogMetadata, userProfiles } from '@/supabase/migrations/schema'

// Every avatar is the DefaultCharacter asset, the character players spawn as, dressed in the player's look.
const DEFAULT_CHARACTER_ASSET_ID = 154

// The hat a player has equipped, and their preferred color as #rrggbb, or null when they haven't picked one.
export type PlayerLook = { equippedItem: { catalogItemId: number, name: string, dataJson: unknown } | null, preferredColor: string | null }

export const getPlayerLook = async (userId: number): Promise<PlayerLook> => {
    const [row] = await db
        .select({ preferredColor: userProfiles.preferredColor, itemId: catalogMetadata.catalogItemId, itemName: catalogMetadata.name })
        .from(userProfiles)
        .leftJoin(catalogMetadata, eq(userProfiles.equippedCatalogItemId, catalogMetadata.catalogItemId))
        .where(eq(userProfiles.userId, userId))
        .limit(1)
    if (row === undefined) return { equippedItem: null, preferredColor: null }
    return {
        equippedItem: row.itemId === null || row.itemName === null ? null : { catalogItemId: row.itemId, name: row.itemName, dataJson: await readCatalogFile(row.itemId) },
        preferredColor: row.preferredColor,
    }
}

export const getDefaultCharacterData = async () => {
    const [asset] = await db
        .select({ assetUuid: assetMetadata.assetUuid })
        .from(assetMetadata)
        .where(eq(assetMetadata.assetId, DEFAULT_CHARACTER_ASSET_ID))
        .orderBy(desc(assetMetadata.versionNum))
        .limit(1)
    if (asset === undefined || asset.assetUuid === null) throw new Error('The DefaultCharacter asset is missing.')
    return readAssetFile(asset.assetUuid)
}
