'use client'

import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Play } from 'lucide-react'
import { z } from 'zod'
import { toast } from 'sonner'
import type { BrowserWgpuRenderer } from '@/lib/gen/render_web/render_web'
import { acquireWgpuRenderResources } from '@/lib/rendering/acquireWgpuRenderResources'
import { AssetDetailLayout } from '@/components/site/AssetDetailLayout'
import { Button } from '@/shadcn/components/ui/button'

// this file mounts render_web

// TODO: AI-PICKED-VALUE: Start above and to the side of the object with a consistent orbit sensitivity across asset previews.
// TODO: AI-PICKED-VALUE: Spin previews look at the object's front almost level with it, so a face on the front is seen head on.
const initialViewOfMode = {
    card: { yaw: 0.55, pitch: 0.6, zoom: 1 },
    detail: { yaw: 0.55, pitch: 0.6, zoom: 1 },
    spin: { yaw: 0, pitch: 0.08, zoom: 1 },
}
// TODO: AI-PICKED-VALUE: A skybox preview faces the middle of its panorama, tilted 0.2 radians up so the horizon sits low in the frame.
const initialSkyView = { yaw: 0, pitch: 0.2, zoom: 1 }
const skyboxDataSchema = z.object({ Skybox: z.number() })
const canvasClassNameOfMode = {
    card: 'block h-full w-full bg-gradient-to-br from-gray-100 to-gray-200',
    detail: 'block aspect-[var(--game-thumbnail-ratio)] w-full touch-none cursor-grab rounded-lg active:cursor-grabbing bg-gradient-to-br from-gray-100 to-gray-200',
    spin: 'block h-full w-full touch-none cursor-grab active:cursor-grabbing',
}
// TODO: AI-PICKED-VALUE: Crossfade selected mesh animations over 250ms using the shared default easing.
const animationTransitionMs = 250

// Cards retain one frame and animate on hover/focus. Detail views also support orbit and zoom. Spin previews draw the
// object with no sky behind it, so it sits on the page itself, and dragging turns it around its up axis. An always
// animated preview, like a profile's avatar, plays without being hovered. An inspectable one can be zoomed, and in a detail view turned to any angle.
export const AssetScenePreview = ({ assetDataJson, assetName, mode, controls, footer, isInspectable, avatarColor = null, isAlwaysAnimated = false }: { assetDataJson: string, assetName: string, mode: 'card' | 'detail' | 'spin', controls: ReactNode, footer: ReactNode, isInspectable: boolean, avatarColor?: string | null, isAlwaysAnimated?: boolean }) => {
    const previewCanvasRef = useRef<HTMLCanvasElement>(null)
    const viewRef = useRef({ ...initialViewOfMode[mode] })
    const elapsedMsRef = useRef(0)
    const [error, setError] = useState<string | null>(null)
    const [isLoading, setIsLoading] = useState(true)
    const [didGpuFail, setDidGpuFail] = useState(false)
    const [skAnimators, setSkAnimators] = useState<{ animator_id: number, skeleton_id: number, animation_name: string, target_weight: number }[]>([])
    const selectSkAnimatorRef = useRef<((animatorId: number) => void) | null>(null)
    const avatarColorRef = useRef(avatarColor)
    const setAvatarColorRef = useRef<((color: string) => void) | null>(null)

    useEffect(() => {
        const previewCanvas = previewCanvasRef.current
        if (!previewCanvas) return
        const isSkybox = skyboxDataSchema.safeParse(JSON.parse(assetDataJson)).success
        const shouldDrawSky = mode !== 'spin'
        const canvas2dContext = previewCanvas.getContext('2d', { alpha: !shouldDrawSky })
        const interactionTarget = mode === 'card' ? previewCanvas.closest('a, button') : previewCanvas
        if (!canvas2dContext || !interactionTarget) throw new Error('Could not initialize the preview canvas.')
        let isDisposed = false
        let didFail = false
        let isReady = false
        let isVisible = false
        let isHovered = interactionTarget.matches(':hover')
        let isFocused = interactionTarget === document.activeElement
        let isDirty = true
        let previousFrameMs: number | undefined
        let wgpuRenderer: BrowserWgpuRenderer | undefined
        let renderResourcesLease: ReturnType<typeof acquireWgpuRenderResources> | undefined
        let renderResources: Awaited<ReturnType<typeof acquireWgpuRenderResources>['resourcesPromise']> | undefined
        let frameId: number | undefined
        let width = 1
        let height = 1
        // The view and the clock stay put when the asset changes, so an avatar keeps its turn, zoom and breathing while trying on hats.
        if (isSkybox) viewRef.current = { ...initialSkyView }

        // All draws finish synchronously before another card can use the shared GPU canvas.
        const shouldAnimate = () => mode === 'detail' || isAlwaysAnimated || isHovered || isFocused
        const stop = () => {
            if (frameId !== undefined) cancelAnimationFrame(frameId)
            frameId = undefined
            previousFrameMs = undefined
        }
        const schedule = () => {
            if (isDisposed || didFail || !isReady || !isVisible || document.hidden) { stop(); return }
            if (frameId === undefined && (isDirty || shouldAnimate())) frameId = requestAnimationFrame(render)
        }
        const render = (nowMs: number) => {
            frameId = undefined
            if (!wgpuRenderer || !renderResources || !isVisible || document.hidden || isDisposed) { stop(); return }
            if (shouldAnimate() && previousFrameMs !== undefined) elapsedMsRef.current += nowMs - previousFrameMs
            previousFrameMs = shouldAnimate() ? nowMs : undefined
            const { yaw, pitch, zoom } = viewRef.current
            try {
                if (renderResources.gpuCanvas.width !== width) renderResources.gpuCanvas.width = width
                if (renderResources.gpuCanvas.height !== height) renderResources.gpuCanvas.height = height
                wgpuRenderer.render(renderResources.browserWgpuRenderContext, width, height, elapsedMsRef.current, yaw, pitch, zoom, shouldDrawSky, mode === 'spin')
                // A see-through frame only covers what it draws, so the last frame is wiped first.
                if (!shouldDrawSky) canvas2dContext.clearRect(0, 0, width, height)
                canvas2dContext.drawImage(renderResources.gpuCanvas, 0, 0)
                isDirty = false
                setIsLoading(false)
                schedule()
            } catch (error) {
                didFail = true
                setError(String(error))
                setIsLoading(false)
            }
        }
        const resizeObserver = new ResizeObserver(([entry]) => {
            // TODO: AI-PICKED-VALUE: Use the same pixel density for cards and details, capped at 2 for sharp Retina previews with bounded GPU cost.
            const ratio = Math.min(window.devicePixelRatio, 2)
            width = Math.max(1, Math.round(entry.contentRect.width * ratio))
            height = Math.max(1, Math.round(entry.contentRect.height * ratio))
            if (previewCanvas.width !== width) previewCanvas.width = width
            if (previewCanvas.height !== height) previewCanvas.height = height
            isDirty = true
            schedule()
        })
        resizeObserver.observe(previewCanvas)
        const visibilityObserver = new IntersectionObserver(([entry]) => {
            isVisible = entry.isIntersecting
            schedule()
        })
        visibilityObserver.observe(previewCanvas)

        const loadPreview = async () => {
            setIsLoading(true)
            setError(null)
            setDidGpuFail(false)
            renderResourcesLease = acquireWgpuRenderResources()
            // Every preview shares one GPU setup, so when it cannot start the page shows one message and the previews stay grey.
            renderResources = await renderResourcesLease.resourcesPromise.catch((error: unknown) => {
                if (!isDisposed && mode !== 'spin') toast.error('3D previews could not start', { description: String(error), id: 'asset-preview-gpu' })
                return undefined
            })
            if (isDisposed) return
            if (!renderResources) { didFail = true; setDidGpuFail(true); setIsLoading(false); return }
            wgpuRenderer = renderResources.renderWeb.BrowserWgpuRenderer.from_asset_data_json(renderResources.browserWgpuRenderContext, assetDataJson, assetName)
            await renderResources.loadAssetsIntoGpu(wgpuRenderer.mesh_data_ids(), wgpuRenderer.texture_data_ids())
            if (isDisposed) return
            isReady = true
            setAvatarColorRef.current = color => {
                if (!wgpuRenderer) return
                try {
                    wgpuRenderer.set_avatar_color(color)
                    isDirty = true
                    schedule()
                } catch (error) {
                    setError(String(error))
                }
            }
            if (avatarColorRef.current !== null) setAvatarColorRef.current(avatarColorRef.current)
            if (mode === 'detail') {
                const readSkAnimators = () => {
                    if (!wgpuRenderer) return
                    setSkAnimators(wgpuRenderer.sk_animators().map(animator => {
                        const { animator_id, skeleton_id, animation_name, target_weight } = animator
                        animator.free()
                        return { animator_id, skeleton_id, animation_name, target_weight }
                    }))
                }
                readSkAnimators()
                selectSkAnimatorRef.current = animatorId => {
                    if (!wgpuRenderer) return
                    try {
                        wgpuRenderer.select_sk_animator(animatorId, animationTransitionMs, elapsedMsRef.current)
                        readSkAnimators()
                        isDirty = true
                        schedule()
                    } catch (error) {
                        setError(String(error))
                    }
                }
            }
            schedule()
        }
        // TODO: AI-PICKED-VALUE: Start asset loading 200px before a card enters the viewport.
        const loadingObserver = new IntersectionObserver(([entry]) => {
            if (!entry.isIntersecting) return
            loadingObserver.disconnect()
            loadPreview().catch(error => {
                if (isDisposed) return
                didFail = true
                setError(String(error))
                setIsLoading(false)
            })
        }, { rootMargin: '200px' })
        loadingObserver.observe(previewCanvas)

        const updateActivity = () => {
            if (!shouldAnimate()) {
                stop()
                elapsedMsRef.current = 0
                isDirty = true
            }
            schedule()
        }
        const enter = () => { isHovered = true; updateActivity() }
        const leave = () => { isHovered = false; updateActivity() }
        const focus = () => { isFocused = true; updateActivity() }
        const blur = () => { isFocused = false; updateActivity() }
        interactionTarget.addEventListener('pointerenter', enter)
        interactionTarget.addEventListener('pointerleave', leave)
        interactionTarget.addEventListener('focus', focus)
        interactionTarget.addEventListener('blur', blur)
        document.addEventListener('visibilitychange', schedule)
        // TODO: AI-PICKED-VALUE: An avatar zooms in until its face fills the preview, and out to half its size.
        const { minZoom, maxZoom } = mode === 'detail' ? { minZoom: 0.05, maxZoom: 100 } : { minZoom: 0.5, maxZoom: 2 }
        const zoom = (event: WheelEvent) => {
            event.preventDefault()
            // TODO: AI-PICKED-VALUE: Exponential zoom stays smooth across small and large asset objects.
            viewRef.current.zoom = Math.max(minZoom, Math.min(maxZoom, viewRef.current.zoom * Math.exp(event.deltaY * 0.001)))
        }
        if (isInspectable) previewCanvas.addEventListener('wheel', zoom, { passive: false })
        return () => {
            isDisposed = true
            selectSkAnimatorRef.current = null
            setAvatarColorRef.current = null
            stop()
            resizeObserver.disconnect()
            visibilityObserver.disconnect()
            loadingObserver.disconnect()
            interactionTarget.removeEventListener('pointerenter', enter)
            interactionTarget.removeEventListener('pointerleave', leave)
            interactionTarget.removeEventListener('focus', focus)
            interactionTarget.removeEventListener('blur', blur)
            document.removeEventListener('visibilitychange', schedule)
            if (isInspectable) previewCanvas.removeEventListener('wheel', zoom)
            wgpuRenderer?.free()
            renderResourcesLease?.release()
        }
    }, [assetDataJson, assetName, mode, isInspectable, isAlwaysAnimated])

    // An avatar takes the color being picked on your own profile without loading again.
    useEffect(() => {
        avatarColorRef.current = avatarColor
        if (avatarColor !== null) setAvatarColorRef.current?.(avatarColor)
    }, [avatarColor])

    const canvas = <canvas ref={previewCanvasRef} aria-label={`3D preview of ${assetName}`} className={`${canvasClassNameOfMode[mode]} transition-opacity duration-300 ${isLoading || error || didGpuFail ? 'opacity-0' : ''}`} onPointerDown={event => {
            if (mode !== 'card') event.currentTarget.setPointerCapture(event.pointerId)
        }} onPointerMove={event => {
            if (mode === 'card' || !event.currentTarget.hasPointerCapture(event.pointerId)) return
            viewRef.current.yaw -= event.movementX * 0.01
            if (isInspectable && mode !== 'spin') viewRef.current.pitch = Math.max(-1.45, Math.min(1.45, viewRef.current.pitch + event.movementY * 0.01))
        }} onPointerUp={event => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
        }} />
    // The canvas stays hidden until it has drawn, so on the detail page a grey tile behind it stands in while it loads.
    const preview = (
        <>
            {mode === 'detail' ? <div className="rounded-lg bg-gradient-to-br from-gray-100 to-gray-200">{canvas}</div> : canvas}
            {isLoading && <p role="status" className="sr-only">Loading preview…</p>}
            {error && mode !== 'spin' && <p role="alert" className={mode === 'card' ? 'absolute inset-x-0 bottom-0 bg-white/90 p-2 text-xs text-destructive' : 'mt-3 text-sm text-destructive'}>{error}</p>}
        </>
    )
    const animations = mode === 'detail' && skAnimators.length > 0 ? (
        <ul className="min-w-0 space-y-1" aria-label="Animations">
            {skAnimators.map(animator => <li key={animator.animator_id}><Button type="button" variant="ghost" className="h-auto min-h-9 w-full justify-start whitespace-normal text-left aria-pressed:bg-accent" aria-pressed={animator.target_weight > 0} disabled={isLoading || error !== null} onClick={() => selectSkAnimatorRef.current?.(animator.animator_id)}><Play className="size-4 fill-current" aria-hidden="true" /><span className="min-w-0 break-words">{animator.animation_name}</span></Button></li>)}
        </ul>
    ) : null

    if (mode === 'card') return <div className="pointer-events-none absolute inset-0">{preview}</div>
    if (mode === 'spin') return preview
    return <AssetDetailLayout preview={preview} controls={<>{animations}{controls}</>} footer={footer} />
}
