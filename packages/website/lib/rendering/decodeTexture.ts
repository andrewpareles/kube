import type initializeBasis from './basis/basis_transcoder.js'

// Keep this in sync with TEXTURE_DATA_CURRENT_VERSION in crates/game_types/src/consts.rs.
const TEXTURE_DATA_CURRENT_VERSION = 1
// TODO: AI-PICKED-VALUE: Cap previews at 1024 pixels, enough for the asset detail card while keeping images small.
export const TEXTURE_PREVIEW_SIZE = 1024

// Match UploadedDataWrapper<TextureData>: version, KTX2 byte count, bytes, three u32s, and two bools.
const readTextureData = (bytes: Uint8Array) => {
    if (bytes.byteLength < 30) throw new Error('Truncated texture asset')
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    if (view.getBigUint64(0, true) !== BigInt(TEXTURE_DATA_CURRENT_VERSION)) throw new Error('Unsupported texture asset version')
    const byteCount = bytes.byteLength - 30
    if (view.getBigUint64(8, true) !== BigInt(byteCount)) throw new Error('Invalid texture asset length')
    const offset = 16 + byteCount
    const width = view.getUint32(offset, true)
    const height = view.getUint32(offset + 4, true)
    const mipLevelCount = view.getUint32(offset + 8, true)
    if (width === 0 || height === 0 || mipLevelCount === 0 || view.getUint8(offset + 12) > 1 || view.getUint8(offset + 13) > 1) throw new Error('Invalid texture metadata')
    return { ktx2Data: bytes.subarray(16, offset), width, height, mipLevelCount, isSrgb: view.getUint8(offset + 12) === 1, hasAlpha: view.getUint8(offset + 13) === 1 }
}

// Basis Universal's transcoder numbers for the WebGPU formats BrowserWgpuRenderContext.texture_upload_format returns.
const basisFormatOfUploadFormat: { [uploadFormat: string]: number | undefined } = { 'rgba8unorm': 13, 'bc1-rgba-unorm': 2, 'bc7-rgba-unorm': 6, 'astc-4x4-unorm': 10 }

// Decode the same KTX2 data for server image previews and browser WebGPU materials.
export const decodeTexture = (bytes: Uint8Array, basis: Awaited<ReturnType<typeof initializeBasis>>, shouldLimitPreviewSize: boolean, chooseUploadFormat: (width: number, height: number, isSrgb: boolean, hasAlpha: boolean) => string) => {
    const texture = readTextureData(bytes)
    const identifier = [0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]
    if (texture.ktx2Data.byteLength < 80 || !identifier.every((byte, index) => texture.ktx2Data[index] === byte)) throw new Error('Invalid KTX2 texture')
    const header = new DataView(texture.ktx2Data.buffer, texture.ktx2Data.byteOffset, texture.ktx2Data.byteLength)
    if (header.getUint32(20, true) !== texture.width || header.getUint32(24, true) !== texture.height || header.getUint32(40, true) !== texture.mipLevelCount || header.getUint32(36, true) !== 1 || header.getUint32(32, true) > 1 || header.getUint32(28, true) > 1) throw new Error('KTX2 texture metadata mismatch')
    const basisFormat = basisFormatOfUploadFormat[chooseUploadFormat(texture.width, texture.height, texture.isSrgb, texture.hasAlpha)]
    if (basisFormat === undefined) throw new Error('Unsupported texture upload format')
    const { KTX2File } = basis
    const file = new KTX2File(texture.ktx2Data)
    try {
        if (!file.startTranscoding()) throw new Error('Could not decode KTX2 texture')
        const firstLevel = shouldLimitPreviewSize ? Math.min(texture.mipLevelCount - 1, Math.max(0, Math.floor(Math.log2(Math.max(texture.width, texture.height) / TEXTURE_PREVIEW_SIZE)))) : 0
        const lastLevel = shouldLimitPreviewSize ? firstLevel + 1 : texture.mipLevelCount
        const mips: Uint8Array[] = []
        for (let level = firstLevel; level < lastLevel; level++) {
            const width = Math.max(1, Math.floor(texture.width / 2 ** level))
            const height = Math.max(1, Math.floor(texture.height / 2 ** level))
            // Basis Universal's cTFRGBA32 format is 13; it writes directly into our own pixel buffer.
            const mip = new Uint8Array(file.getImageTranscodedSizeInBytes(level, 0, 0, basisFormat))
            if (!file.transcodeImage(mip, level, 0, 0, basisFormat, 0, -1, -1)) throw new Error('Could not decode KTX2 pixels')
            mips.push(mip)
        }
        return { width: Math.max(1, Math.floor(texture.width / 2 ** firstLevel)), height: Math.max(1, Math.floor(texture.height / 2 ** firstLevel)), isSrgb: texture.isSrgb, hasAlpha: texture.hasAlpha, mips }
    } finally {
        file.close()
        file.delete()
    }
}

export const decodeTextureToGpuBytes = (bytes: Uint8Array, basis: Awaited<ReturnType<typeof initializeBasis>>, chooseUploadFormat: (width: number, height: number, isSrgb: boolean, hasAlpha: boolean) => string) => {
    const { width, height, isSrgb, hasAlpha, mips } = decodeTexture(bytes, basis, false, chooseUploadFormat)
    // BrowserWgpuRenderContext::load_texture_bytes_into_gpu reads this header and the contiguous mip levels.
    const result = new Uint8Array(20 + mips.reduce((size, mip) => size + mip.byteLength, 0))
    const header = new DataView(result.buffer)
    header.setUint32(0, width, true)
    header.setUint32(4, height, true)
    header.setUint32(8, mips.length, true)
    header.setUint32(12, isSrgb ? 1 : 0, true)
    header.setUint32(16, hasAlpha ? 1 : 0, true)
    let offset = 20
    for (const mip of mips) { result.set(mip, offset); offset += mip.byteLength }
    return result
}
