import { initializeWgpuRenderer } from '@/lib/rendering/initializeWgpuRenderer'
import { decodeBrowserTextureToGpuBytes } from '@/lib/rendering/decodeBrowserTextureToGpuBytes'
import { urlOfPublicFile } from '@/lib/s3StorageFiles'
import { toast } from 'sonner'

// A page shares one renderer and loads each asset into the GPU once. Each visible canvas keeps its own last frame.
export const createWgpuRenderResources = async (signal: AbortSignal) => {
    const renderWeb = await initializeWgpuRenderer()
    signal.throwIfAborted()
    const gpuCanvas = document.createElement('canvas')
    gpuCanvas.width = 1
    gpuCanvas.height = 1
    const browserWgpuRenderContext = await renderWeb.BrowserWgpuRenderContext.new(gpuCanvas)
    if (signal.aborted) { browserWgpuRenderContext.free(); signal.throwIfAborted() }
    const gpuLoadOfUrl: { [url: string]: Promise<void> | undefined } = {}
    const loadAssetIntoGpu = (url: string, dataId: bigint, kind: 'mesh' | 'texture') => {
        // Share pending and successful uploads. A failure belongs to this asset, never to the scene.
        gpuLoadOfUrl[url] ??= fetch(url, { signal }).then(async response => {
            if (!response.ok) throw new Error(`Download failed (HTTP ${response.status}).`)
            const bytes = new Uint8Array(await response.arrayBuffer())
            signal.throwIfAborted()
            if (kind === 'mesh') browserWgpuRenderContext.load_mesh_bytes_into_gpu(dataId, bytes)
            else {
                const textureBytes = await decodeBrowserTextureToGpuBytes(bytes, (width, height, isSrgb, hasAlpha) => browserWgpuRenderContext.texture_upload_format(width, height, isSrgb, hasAlpha))
                signal.throwIfAborted()
                browserWgpuRenderContext.load_texture_bytes_into_gpu(dataId, textureBytes)
            }
        }).catch(error => {
            delete gpuLoadOfUrl[url]
            if (!signal.aborted) toast.error(`Could not load ${kind} Data ${dataId}`, { description: String(error), id: `render-asset-${kind}-${dataId}` })
        })
        return gpuLoadOfUrl[url]
    }
    return {
        renderWeb,
        gpuCanvas,
        browserWgpuRenderContext,
        // Screenshots and static previews can await completion; interactive views can draw during loading.
        // Missing assets use the renderer's existing handling, and a later request can retry a failed upload.
        loadAssetsIntoGpu: (meshDataIds: BigInt64Array, textureDataIds: BigInt64Array) => Promise.all([
            ...Array.from(meshDataIds, dataId => loadAssetIntoGpu(urlOfPublicFile({ kind: 'Data', dataKind: 'mesh', dataId: Number(dataId) }), dataId, 'mesh')),
            ...Array.from(textureDataIds, dataId => loadAssetIntoGpu(urlOfPublicFile({ kind: 'Data', dataKind: 'texture', dataId: Number(dataId) }), dataId, 'texture')),
        ]),
    }
}
