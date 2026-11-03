import 'server-only'

import { createAssetTexturePreview } from '@/app/assets/[asset_id]/texture/createAssetTexturePreview'
import { readSoundData } from '@/app/assets/[asset_id]/sound/readSoundData'
import { urlOfPublicFile } from '@/lib/s3StorageFiles'

export type DataKind = 'mesh' | 'texture' | 'sound'

// The largest Ogg page: a 27-byte header, 255 segment sizes, and 255 segments of 255 bytes.
const MAX_OGG_PAGE_BYTES = 27 + 255 + 255 * 255
// Keep this in sync with MESH_DATA_CURRENT_VERSION in crates/game_types/src/consts.rs.
const MESH_DATA_CURRENT_VERSION = 6

const getDataUrl = (dataKind: DataKind, dataId: number) => urlOfPublicFile({ kind: 'Data', dataKind, dataId })

export const readDataId = (dataIdParam: string) => {
    const dataId = Number(dataIdParam)
    return Number.isSafeInteger(dataId) && dataId > 0 ? dataId : null
}

// A mesh file holds its version, whether it is skinned, its vertices, and its triangles' corners, then a skinned mesh's
// joint names. Every vertex starts with its position, and a skinned vertex also carries four joints and their weights.
// Older versions are laid out differently and the engine no longer reads them, so they get no numbers.
export const readMeshInfo = async (dataId: number) => {
    const response = await fetch(getDataUrl('mesh', dataId))
    if (!response.ok) return null
    const bytes = Buffer.from(await response.arrayBuffer())
    const version = Number(bytes.readBigUInt64LE(0))
    if (version !== MESH_DATA_CURRENT_VERSION) return { version, stats: null }
    const isSkinned = bytes.readUInt32LE(8) === 0
    const vertexByteCount = isSkinned ? 80 : 48
    const vertexCount = Number(bytes.readBigUInt64LE(12))
    const min = [Infinity, Infinity, Infinity]
    const max = [-Infinity, -Infinity, -Infinity]
    for (let vertex = 0; vertex < vertexCount; vertex++) {
        for (let axis = 0; axis < 3; axis++) {
            const position = bytes.readFloatLE(20 + vertex * vertexByteCount + axis * 4)
            min[axis] = Math.min(min[axis], position)
            max[axis] = Math.max(max[axis], position)
        }
    }
    const indicesStart = 20 + vertexCount * vertexByteCount
    const indexCount = Number(bytes.readBigUInt64LE(indicesStart))
    const jointCount = isSkinned ? Number(bytes.readBigUInt64LE(indicesStart + 8 + indexCount * 4)) : null
    return { version, stats: { vertexCount, triangleCount: indexCount / 3, size: vertexCount === 0 ? [0, 0, 0] : max.map((value, axis) => value - min[axis]), jointCount } }
}

// A texture file ends with its width, height, and mip count, then whether it is color and whether it has alpha, so only
// those last 14 bytes are downloaded.
export const readTextureInfo = async (dataId: number) => {
    const response = await fetch(getDataUrl('texture', dataId), { headers: { Range: 'bytes=-14' } })
    if (!response.ok) return null
    const bytes = Buffer.from(await response.arrayBuffer())
    if (bytes.byteLength !== 14) throw new Error('Invalid texture asset')
    return { width: bytes.readUInt32LE(0), height: bytes.readUInt32LE(4), isSrgb: bytes.readUInt8(12) === 1 }
}

// Only the two ends of a sound file are downloaded. After the file's version and byte count, the first Ogg page holds the
// Vorbis header with the channel count and sample rate, and the last page's position is the sound's length in samples.
export const readSoundInfo = async (dataId: number) => {
    const url = getDataUrl('sound', dataId)
    const [head, tail] = await Promise.all([fetch(url, { headers: { Range: 'bytes=0-127' } }), fetch(url, { headers: { Range: `bytes=-${MAX_OGG_PAGE_BYTES}` } })])
    if (!head.ok || !tail.ok) return null
    const headBytes = Buffer.from(await head.arrayBuffer())
    const tailBytes = Buffer.from(await tail.arrayBuffer())
    const packetStart = 16 + 27 + headBytes.readUInt8(16 + 26)
    const lastPageStart = tailBytes.lastIndexOf('OggS')
    if (headBytes.toString('latin1', packetStart, packetStart + 7) !== '\x01vorbis' || lastPageStart < 0 || lastPageStart + 14 > tailBytes.byteLength) throw new Error('Invalid Ogg Vorbis sound')
    const sampleRate = headBytes.readUInt32LE(packetStart + 12)
    return { channelCount: headBytes.readUInt8(packetStart + 11), sampleRate, durationSeconds: Number(tailBytes.readBigInt64LE(lastPageStart + 6)) / sampleRate }
}

// Asset and data pages send the same texture preview and sound, read from the stored file.
export const respondWithTexturePreview = async (request: Request, dataId: number) => {
    const headers = { 'ETag': `"texture-${dataId}"`, 'Cache-Control': 'public, no-cache' }
    if (request.headers.get('If-None-Match') === headers.ETag) return new Response(null, { status: 304, headers })
    const response = await fetch(getDataUrl('texture', dataId))
    if (!response.ok) return new Response(null, { status: 502 })
    const preview = await createAssetTexturePreview(new Uint8Array(await response.arrayBuffer()))
    return new Response(new Uint8Array(preview), { headers: { ...headers, 'Content-Type': 'image/png', 'Content-Length': String(preview.byteLength) } })
}

export const respondWithSound = async (dataId: number) => {
    const response = await fetch(getDataUrl('sound', dataId))
    if (!response.ok) return new Response(null, { status: 502 })
    const sound = readSoundData(new Uint8Array(await response.arrayBuffer()))
    return new Response(sound, { headers: { 'Content-Type': 'audio/ogg', 'Content-Length': String(sound.byteLength) } })
}
