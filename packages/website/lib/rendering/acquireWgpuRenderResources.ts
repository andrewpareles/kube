import { createWgpuRenderResources } from '@/lib/rendering/createWgpuRenderResources'

type WgpuRenderResources = Awaited<ReturnType<typeof createWgpuRenderResources>>
let sharedSession: {
    resourcesPromise: Promise<WgpuRenderResources>,
    abortController: AbortController,
    userCount: number
} | undefined

// Release only after the last preview unmounts, including when GPU initialization is still pending.
export const acquireWgpuRenderResources = () => {
    if (!sharedSession) {
        const abortController = new AbortController()
        sharedSession = { resourcesPromise: createWgpuRenderResources(abortController.signal), abortController, userCount: 0 }
    }
    const acquiredSession = sharedSession
    acquiredSession.userCount++
    let isReleased = false
    return {
        resourcesPromise: acquiredSession.resourcesPromise,
        release: () => {
            if (isReleased) return
            isReleased = true
            acquiredSession.userCount--
            if (acquiredSession.userCount !== 0) return
            sharedSession = undefined
            acquiredSession.abortController.abort()
            // Initialization errors are reported by the previews; disposal still handles a pending success.
            acquiredSession.resourcesPromise.then(({ browserWgpuRenderContext }) => browserWgpuRenderContext.free(), () => { })
        },
    }
}
