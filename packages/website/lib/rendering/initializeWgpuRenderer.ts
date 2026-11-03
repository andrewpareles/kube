import * as renderWebForWebGpu from '@/lib/gen/render_web/render_web'

// TypeScript's DOM types do not include WebGPU yet, and this file only needs to know whether the browser has an adapter.
declare global {
    interface Navigator {
        readonly gpu?: { requestAdapter: () => Promise<object | null> }
    }
}

// Browsers that give WebGPU no adapter draw with the WebGL2 build, which is bigger, so only they download it.
const loadRenderWeb = async () => {
    const webGpuAdapter = await navigator.gpu?.requestAdapter()
    const renderWeb = webGpuAdapter ? renderWebForWebGpu : await import('@/lib/gen/render_web_webgl/render_web')
    await renderWeb.default()
    return renderWeb
}

let initialization: ReturnType<typeof loadRenderWeb> | undefined
export const initializeWgpuRenderer = () => { initialization ??= loadRenderWeb().catch(error => { initialization = undefined; throw error }); return initialization }
