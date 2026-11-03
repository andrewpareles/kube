import type { DataKind } from '@/lib/siteData'

// The website reads only public files, which CloudFront serves to anyone at cdn.kube.new. A file named by an integer id is found
// from the id alone; a file named by a uuid is found through the row that points at it. Every file is written once and never
// changed. Gzipped files are unzipped by fetch on the way in. The full list, with the private files only our servers read, is in
// crates/shared/src/s3_storage.rs.
// Tests serve files from a local folder instead.
export const CDN_ORIGIN = process.env.NEXT_PUBLIC_CDN_URL ?? 'https://cdn.kube.new'

export type PublicFile =
    // Gzipped for meshes only. Textures and sounds are stored as they are.
    | { kind: 'Data', dataKind: DataKind, dataId: number }
    // Gzipped.
    | { kind: 'Catalog', catalogItemId: number }
    // Not gzipped.
    | { kind: 'GameThumbnail', thumbnailUuid: string }
    // Gzipped.
    | { kind: 'Asset', assetUuid: string }

export const keyOfPublicFile = (file: PublicFile) => {
    switch (file.kind) {
        case 'Data': return `data/${file.dataKind}_data/${file.dataId}`
        case 'Catalog': return `catalog/${file.catalogItemId}`
        case 'GameThumbnail': return `game-thumbnails/${file.thumbnailUuid}`
        case 'Asset': return `assets/${file.assetUuid}`
    }
}

export const urlOfPublicFile = (file: PublicFile) => `${CDN_ORIGIN}/${keyOfPublicFile(file)}`

// A game without a thumbnail has no uuid, so it has no URL either.
export const urlOfGameThumbnail = (thumbnailUuid: string | null) => thumbnailUuid === null ? null : urlOfPublicFile({ kind: 'GameThumbnail', thumbnailUuid })
