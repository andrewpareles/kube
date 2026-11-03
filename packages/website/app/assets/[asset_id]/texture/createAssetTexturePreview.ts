import { readFile } from 'node:fs/promises'
import path from 'node:path'
import initializeBasis from '../../../../lib/rendering/basis/basis_transcoder.js'
import { decodeTexture, TEXTURE_PREVIEW_SIZE } from '../../../../lib/rendering/decodeTexture.ts'

let basis: ReturnType<typeof initializeBasis> | undefined

// Asset image responses use small PNGs; the browser renderer decodes full mip chains directly.
export const createAssetTexturePreview = async (bytes: Uint8Array) => {
    basis ??= readFile(path.join(process.cwd(), 'lib/rendering/basis/basis_transcoder.wasm')).then(wasmBinary => initializeBasis({ wasmBinary })).then(module => {
        module.initializeBasis()
        return module
    })
    const { default: sharp } = await import('sharp')
    const { width, height, mips } = decodeTexture(bytes, await basis, true, () => 'rgba8unorm')
    return sharp(mips[0], { raw: { width, height, channels: 4 } }).resize({ width: TEXTURE_PREVIEW_SIZE, height: TEXTURE_PREVIEW_SIZE, fit: 'inside', withoutEnlargement: true }).png().toBuffer()
}
