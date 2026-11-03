import 'server-only'

import { and, count, desc, eq, inArray, sql } from 'drizzle-orm'
import { db } from '@/lib/db'
import { urlOfPublicFile } from '@/lib/s3StorageFiles'
import { catalogMetadata, userOwnedCatalogItems, userProfiles } from '@/supabase/migrations/schema'

// TODO: AI-PICKED-VALUE: Show 24 items per page, like the Assets page.
export const CATALOG_PAGE_SIZE = 24

const catalogItemsMatching = (query: string) => and(
    eq(catalogMetadata.isListed, true),
    sql`strpos(lower(${catalogMetadata.name}), lower(${query})) > 0`,
)

// An item's data is its own file on the CDN, named by its id.
export const readCatalogFile = async (catalogItemId: number): Promise<unknown> => {
    const response = await fetch(urlOfPublicFile({ kind: 'Catalog', catalogItemId }))
    if (!response.ok) throw new Error(`Could not read catalog item ${catalogItemId} (HTTP ${response.status}).`)
    return response.json()
}

const withCatalogData = <Row extends { catalogItemId: number }>(rows: Row[]) => Promise.all(rows.map(async row => ({
    ...row,
    dataJson: await readCatalogFile(row.catalogItemId),
})))

const catalogCardColumns = {
    catalogItemId: catalogMetadata.catalogItemId,
    name: catalogMetadata.name,
    creatorUsername: userProfiles.username,
}

// Someone who isn't signed in owns nothing.
const isItemOwnedBy = (viewerId: string | null) => viewerId === null
    ? sql<boolean>`false`
    : sql<boolean>`exists (select 1 from ${userOwnedCatalogItems} where ${userOwnedCatalogItems.catalogItemId} = ${catalogMetadata.catalogItemId} and ${userOwnedCatalogItems.profileId} = ${viewerId})`

export const getCatalogItems = async ({ query, offset, limit, viewerId }: { query: string, offset: number, limit: number, viewerId: string | null }) => withCatalogData(await db
    .select({ ...catalogCardColumns, isOwnedByViewer: isItemOwnedBy(viewerId) })
    .from(catalogMetadata)
    .innerJoin(userProfiles, eq(catalogMetadata.creatorId, userProfiles.id))
    .where(catalogItemsMatching(query))
    .orderBy(desc(catalogMetadata.createdAt), desc(catalogMetadata.catalogItemId))
    .offset(offset)
    .limit(limit))

export const countCatalogItems = async ({ query }: { query: string }) => {
    const [{ itemCount }] = await db.select({ itemCount: count() }).from(catalogMetadata).where(catalogItemsMatching(query))
    return itemCount
}

// The items a profile made. Its creator also sees the ones that are not public.
const profileCatalogItemsMatching = ({ userId, isOwner }: { userId: number, isOwner: boolean }) =>
    and(eq(userProfiles.userId, userId), isOwner ? undefined : eq(catalogMetadata.isListed, true))

export const getProfileCatalogItems = async ({ userId, isOwner }: { userId: number, isOwner: boolean }) => withCatalogData(await db
    .select(catalogCardColumns)
    .from(catalogMetadata)
    .innerJoin(userProfiles, eq(catalogMetadata.creatorId, userProfiles.id))
    .where(profileCatalogItemsMatching({ userId, isOwner }))
    .orderBy(desc(catalogMetadata.createdAt), desc(catalogMetadata.catalogItemId)))

// The items a profile owns, the newest purchase first.
export const getProfileOwnedCatalogItems = async (userId: number) => withCatalogData(await db
    .select(catalogCardColumns)
    .from(userOwnedCatalogItems)
    .innerJoin(catalogMetadata, eq(userOwnedCatalogItems.catalogItemId, catalogMetadata.catalogItemId))
    .innerJoin(userProfiles, eq(catalogMetadata.creatorId, userProfiles.id))
    .where(inArray(userOwnedCatalogItems.profileId, db.select({ id: userProfiles.id }).from(userProfiles).where(eq(userProfiles.userId, userId))))
    .orderBy(desc(userOwnedCatalogItems.purchasedAt)))

// A private item is still shown to its creator and to the people who own it.
export const getCatalogItem = async ({ catalogItemId, viewerId }: { catalogItemId: number, viewerId: string | null }) => {
    const [item] = await db
        .select({
            catalogItemId: catalogMetadata.catalogItemId,
            creatorId: catalogMetadata.creatorId,
            name: catalogMetadata.name,
            description: catalogMetadata.description,
            isListed: catalogMetadata.isListed,
            creatorUserId: userProfiles.userId,
            creatorUsername: userProfiles.username,
            isOwnedByViewer: isItemOwnedBy(viewerId),
        })
        .from(catalogMetadata)
        .innerJoin(userProfiles, eq(catalogMetadata.creatorId, userProfiles.id))
        .where(eq(catalogMetadata.catalogItemId, catalogItemId))
        .limit(1)
    if (item === undefined) return null
    if (!item.isListed && item.creatorId !== viewerId && !item.isOwnedByViewer) return null
    return { ...item, dataJson: await readCatalogFile(item.catalogItemId) }
}
