import type initializeBasis from './basis/basis_transcoder.js'
import { decodeTextureToGpuBytes } from './decodeTexture'

// The unmodified Basis wrapper exposes this factory when loaded as a classic browser script.
declare const BASIS: typeof initializeBasis
let basisInitialization: ReturnType<typeof initializeBasis> | undefined

export const decodeBrowserTextureToGpuBytes = async (bytes: Uint8Array, chooseUploadFormat: (width: number, height: number, isSrgb: boolean, hasAlpha: boolean) => string) => {
    basisInitialization ??= Promise.all([
        fetch(new URL('./basis/basis_transcoder.wasm', import.meta.url)).then(async response => {
            if (!response.ok) throw new Error(`Could not load the texture decoder (${response.status}).`)
            return new Uint8Array(await response.arrayBuffer())
        }),
        new Promise<void>((resolve, reject) => {
            const script = document.createElement('script')
            script.src = new URL('./basis/basis_transcoder.js', import.meta.url).href
            script.onload = () => { script.remove(); resolve() }
            script.onerror = () => { script.remove(); reject(new Error('Could not load the texture decoder.')) }
            document.head.append(script)
        }),
    ]).then(async ([wasmBinary]) => {
        const basis = await BASIS({ wasmBinary })
        basis.initializeBasis()
        return basis
    }).catch(error => {
        basisInitialization = undefined
        throw error
    })
    return decodeTextureToGpuBytes(bytes, await basisInitialization, chooseUploadFormat)
}
