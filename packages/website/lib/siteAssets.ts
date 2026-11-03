import 'server-only'

import { and, count, desc, eq, sql } from 'drizzle-orm'
import { z } from 'zod'
import { db } from '@/lib/db'
import { assetMetadata, userProfiles } from '@/supabase/migrations/schema'
import type { AssetType } from '@/lib/assetTypes'
import { urlOfPublicFile } from '@/lib/s3StorageFiles'
import type { DataKind } from '@/lib/siteData'

export const ASSETS_PAGE_SIZE = 20
export const assetTypeNames: Record<AssetType, string> = { Mesh: 'Mesh', Texture: 'Texture', Sound: 'Sound', Object: 'Object', Material: 'Material', Skybox: 'Skybox', Sfx: 'SFX' }

// Read the fields the website displays from the normal Serde JSON shape.
const assetDetailsSchema = z.union([
    z.object({ Texture: z.number().int() }),
    z.object({ Sound: z.number().int() }),
    z.object({ Skybox: z.number().int() }),
    z.object({ Sfx: z.number().int() }),
])

export const readAssetDetails = (data: unknown) => assetDetailsSchema.parse(data)

type DataReference = { dataKind: DataKind, dataId: number }
const dataKinds: DataKind[] = ['mesh', 'texture', 'sound']

// Models and stored objects keep each data id under the same field names at any depth, so the whole payload is searched.
const collectDataReferences = (value: unknown, references: DataReference[]) => {
    if (Array.isArray(value)) { for (const item of value) collectDataReferences(item, references); return }
    if (typeof value !== 'object' || value === null) return
    for (const [field, child] of Object.entries(value)) {
        if (typeof child !== 'number') collectDataReferences(child, references)
        else if (field === 'mesh_data_id' || field === 'meshDataId') references.push({ dataKind: 'mesh', dataId: child })
        else if (field === 'soundDataId') references.push({ dataKind: 'sound', dataId: child })
        else if (field.endsWith('TextureDataId')) references.push({ dataKind: 'texture', dataId: child })
    }
}

// Every data file an asset uses, once each, meshes first, then textures, then sounds.
export const getAssetDataReferences = (data: unknown) => {
    const details = assetDetailsSchema.safeParse(data)
    const references: DataReference[] = []
    if (!details.success) collectDataReferences(data, references)
    else if ('Texture' in details.data) references.push({ dataKind: 'texture', dataId: details.data.Texture })
    else if ('Skybox' in details.data) references.push({ dataKind: 'texture', dataId: details.data.Skybox })
    else if ('Sfx' in details.data) references.push({ dataKind: 'sound', dataId: details.data.Sfx })
    else references.push({ dataKind: 'sound', dataId: details.data.Sound })
    return references.filter((reference, index) => references.findIndex(other => other.dataKind === reference.dataKind && other.dataId === reference.dataId) === index).toSorted((a, b) => dataKinds.indexOf(a.dataKind) - dataKinds.indexOf(b.dataKind))
}

// Each asset version's data is its own file on the CDN, named by the uuid its row keeps.
export const readAssetFile = async (assetUuid: string): Promise<unknown> => {
    const response = await fetch(urlOfPublicFile({ kind: 'Asset', assetUuid }))
    if (!response.ok) throw new Error(`Could not read asset file ${assetUuid} (HTTP ${response.status}).`)
    return response.json()
}

const withAssetData = <Row extends { assetUuid: string | null }>(rows: Row[]) => Promise.all(rows.map(async ({ assetUuid, ...row }) => ({
    ...row,
    dataJson: assetUuid === null ? null : await readAssetFile(assetUuid),
})))

// Choose the current revision before filtering or paging so each asset appears once.
const getLatestAssets = () => db.selectDistinctOn([assetMetadata.assetId], {
    assetId: assetMetadata.assetId,
    versionNum: assetMetadata.versionNum,
    name: assetMetadata.name,
    assetType: assetMetadata.assetType,
    createdAt: assetMetadata.createdAt,
    uploadedBy: assetMetadata.uploadedBy,
    assetUuid: assetMetadata.assetUuid,
    importCount: sql<number>`sum(${assetMetadata.importCount}) over (partition by ${assetMetadata.assetId})`.mapWith(Number).as('import_count'),
}).from(assetMetadata).orderBy(assetMetadata.assetId, desc(assetMetadata.versionNum)).as('latest')

const getMatchingLatestAssets = ({ query, assetType }: { query: string, assetType: AssetType | undefined }) => {
    const latest = getLatestAssets()
    const where = and(
        sql`strpos(lower(${latest.name}), lower(${query})) > 0`, // string search
        assetType === undefined ? undefined : eq(latest.assetType, assetType),
    )
    return { latest, where }
}

export const getAssets = async ({ query, assetType, offset, limit }: { query: string, assetType: AssetType | undefined, offset: number, limit: number }) => {
    const { latest, where } = getMatchingLatestAssets({ query, assetType })
    return withAssetData(await db.select().from(latest).where(where).orderBy(desc(latest.createdAt), desc(latest.assetId)).offset(offset).limit(limit))
}

export const countAssets = async ({ query, assetType }: { query: string, assetType: AssetType | undefined }) => {
    const { latest, where } = getMatchingLatestAssets({ query, assetType })
    const [{ assetCount }] = await db.select({ assetCount: count() }).from(latest).where(where)
    return assetCount
}

// The current revision of every asset a profile uploaded, newest first.
export const getProfileAssets = async ({ userId, offset, limit }: { userId: number, offset: number, limit: number }) => {
    const latest = getLatestAssets()
    return withAssetData(await db
        .select({ assetId: latest.assetId, name: latest.name, assetType: latest.assetType, assetUuid: latest.assetUuid, importCount: latest.importCount })
        .from(latest)
        .innerJoin(userProfiles, eq(latest.uploadedBy, userProfiles.id))
        .where(eq(userProfiles.userId, userId))
        .orderBy(desc(latest.createdAt), desc(latest.assetId))
        .offset(offset)
        .limit(limit))
}

export const countProfileAssets = async ({ userId }: { userId: number }) => {
    const latest = getLatestAssets()
    const [{ assetCount }] = await db
        .select({ assetCount: count() })
        .from(latest)
        .innerJoin(userProfiles, eq(latest.uploadedBy, userProfiles.id))
        .where(eq(userProfiles.userId, userId))
    return assetCount
}

export const getAsset = async ({ assetId }: { assetId: number }) => {
    const assets = await db.select({
        assetId: assetMetadata.assetId,
        versionNum: assetMetadata.versionNum,
        name: assetMetadata.name,
        createdAt: assetMetadata.createdAt,
        uploadedBy: assetMetadata.uploadedBy,
        assetType: assetMetadata.assetType,
        assetUuid: assetMetadata.assetUuid,
        uploaderUserId: userProfiles.userId,
        uploaderUsername: userProfiles.username,
    }).from(assetMetadata).innerJoin(userProfiles, eq(assetMetadata.uploadedBy, userProfiles.id)).where(eq(assetMetadata.assetId, assetId)).orderBy(desc(assetMetadata.versionNum)).limit(1)
    const [asset] = await withAssetData(assets)
    return asset ?? null
}
